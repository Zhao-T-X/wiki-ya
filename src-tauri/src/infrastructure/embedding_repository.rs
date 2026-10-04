//! 语义检索的向量存储（Phase 6）。
//!
//! 设计要点：
//! - `chunk_embeddings` 是派生数据（0002 迁移），chunk 删除即失效，重算即可恢复。
//! - embedding 以 little-endian f32 序列化成 `BLOB` 落库，读取时反向还原。
//! - `nearest_chunks` 做暴力余弦相似度（库规模小，无需 ANN 索引），诚实返回真实分数；
//!   无任何伪造/兜底分数（TDD「诚实优先」）。
//! - PERF-01/02：向量化只补「当前模型下缺向量」的 chunk（[`chunks_missing_embedding`]），
//!   检索侧只保留 top_k（有界堆 + 两阶段取正文），均不再随库规模线性变慢。
//! - 所有函数失败都通过 `AppResult` 上抛；调用方（semantic_search）决定如何降级。

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use crate::ai::context::ContextKind;
use crate::ai::segmentation::Segment;
use crate::application::retrieval_service::RetrievedPassage;
use crate::error::AppResult;

/// 把一个 f32 向量序列化成 little-endian 字节（与下游反序列化严格对应）。
fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// 把 little-endian 字节还原成 f32 向量。长度不是 4 的倍数时丢弃尾部残字节。
fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// 存储（或覆盖）单个 chunk **某一段**的 embedding（PERF-07）。
///
/// 主键是 `(chunk_id, model, part)`：
/// - `part = 0` 且 `segment = None` → 短 chunk 未切分，与改造前完全等价；
/// - `part > 0` → 超上下文窗口的长 chunk 切段，每段一行向量。
/// - **多模型共存**：同一 chunk 可同时持有不同模型的向量，切换模型不再全库重算。
///
/// `segment_text` 存**实际送去嵌入的那段原文**：检索返回的证据只能是它。
/// 若向量只比较了前半段却展示整块，就是虚报证据范围。
pub fn store_embedding(
    conn: &Connection,
    chunk_id: &str,
    model: &str,
    embedding: &[f32],
    part: usize,
    segment: Option<&Segment>,
) -> AppResult<()> {
    let blob = serialize_embedding(embedding);
    let dimensions = embedding.len() as i64;
    let (text, start, end) = match segment {
        Some(s) => (Some(s.text.clone()), Some(s.char_start as i64), Some(s.char_end as i64)),
        None => (None, None, None),
    };
    conn.execute(
        "INSERT INTO chunk_embeddings
           (chunk_id, model, part, embedding, dimensions, segment_text, char_start, char_end, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))
         ON CONFLICT(chunk_id, model, part) DO UPDATE SET
           embedding = excluded.embedding,
           dimensions = excluded.dimensions,
           segment_text = excluded.segment_text,
           char_start = excluded.char_start,
           char_end = excluded.char_end,
           created_at = datetime('now')",
        params![chunk_id, model, part as i64, blob, dimensions, text, start, end],
    )?;
    Ok(())
}

/// 读取单个 chunk 的 embedding；不存在时返回 `None`（不报错）。
pub fn load_embedding(conn: &Connection, chunk_id: &str) -> AppResult<Option<Vec<f32>>> {
    let result: Option<Vec<u8>> = conn
        .query_row(
            "SELECT embedding FROM chunk_embeddings WHERE chunk_id = ?1",
            params![chunk_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    Ok(result.map(|bytes| deserialize_embedding(&bytes)))
}

/// 读取「当前 embedding 模型下**还没有向量**」的 chunk 文本：(chunk_id, content)。
///
/// PERF-01：这是「只补缺失、不重算全库」的关键。语义检索每次只向量化
/// 这里返回的条目，其余 chunk 直接复用已有向量。
///
/// 之所以不需要额外的 content 指纹：`replace_chunks` 是 DELETE 后以**全新 id**
/// 插入，所以「内容变更 ⇒ chunk id 变更 ⇒ 该 id 没有向量行」，天然被此查询覆盖。
/// （若将来改成稳定 id 的增量 reindex，必须同时补指纹，否则会读到过期向量。）
pub fn chunks_missing_embedding(
    conn: &Connection,
    model: &str,
) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.content
         FROM chunks c
         LEFT JOIN chunk_embeddings e
                ON e.chunk_id = c.id AND e.model = ?1
         WHERE e.chunk_id IS NULL",
    )?;
    let rows = stmt.query_map(params![model], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 库里是否存在任何 chunk。
///
/// PERF-01：空库时直接降级，不必为 query 白付一次 embedding 网络调用。
pub fn has_chunks(conn: &Connection) -> AppResult<bool> {
    let exists: i64 = conn.query_row("SELECT EXISTS(SELECT 1 FROM chunks)", [], |r| r.get(0))?;
    Ok(exists != 0)
}

/// 余弦相似度：无归一化或维度不匹配时返回 0.0（诚实，而非捏造）。
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..n {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// f32 不实现 `Ord`，用「无法比较时视为相等」的全序包装，使 `BinaryHeap` 可用。
/// 不影响诚实性：余弦已由 [`cosine_similarity`] 把零向量/维度不匹配归一为 0.0。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Score(f32);

impl Eq for Score {}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Score {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// 某个 chunk 命中后的分段信息（PERF-07）。
#[derive(Debug, Clone)]
struct Hit {
    score: f32,
    part: usize,
    /// 该段实际参与嵌入的文本；`None` 表示未切分（整块参与）。
    segment_text: Option<String>,
    char_span: Option<(usize, usize)>,
}

/// 单次取正文的 id 上限：SQLite 绑定变量上限 999，留余量。
const ID_BATCH: usize = 900;

/// 对库内 chunk embedding 做暴力余弦近邻，返回 top_k。PERF-02。
///
/// 与旧实现的三点差异（都是为了不再随库规模线性变慢）：
/// 1. **有界堆**：只保留 top_k，复杂度 `O(N·D + N·log K)`、内存 `O(K)`，
///    不再把 N 条结果全塞进 `Vec` 后全量排序；
/// 2. **两阶段取正文**：第一阶段只读 `chunk_id + embedding`（不 JOIN 正文），
///    算出 top_k 之后才按 id 取回原文——避免 N 份正文进内存；
/// 3. **按 model 过滤**：命中 `idx_chunk_embeddings_model`（0002 建了但此前没被用上）。
///
/// 分数即真实余弦相似度（[-1,1] 区间），绝不伪造。
/// 若 embedding 行的 `chunk_id` 在 `chunks` 中已不存在（悬挂行），该条会被跳过——
/// 等价于旧实现的 INNER JOIN 语义。
pub fn nearest_chunks(
    conn: &Connection,
    model: &str,
    query_vec: &[f32],
    top_k: usize,
) -> AppResult<Vec<RetrievedPassage>> {
    if top_k == 0 {
        return Ok(Vec::new());
    }

    // ---- 阶段一：扫向量，用有界小顶堆保留候选 ----
    // 过量取（top_k * OVERFETCH）：长 chunk 的多段可能挤占名额，去重后仍要凑够 top_k。
    const OVERFETCH: usize = 3;
    let want = top_k.saturating_mul(OVERFETCH).max(top_k);

    let mut stmt = conn.prepare(
        "SELECT chunk_id, part, embedding, segment_text, char_start, char_end
         FROM chunk_embeddings WHERE model = ?1",
    )?;
    let rows = stmt.query_map(params![model], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    })?;

    // 小顶堆：堆顶是当前候选里最弱的一条，新条目更强就替换它。
    let mut heap: BinaryHeap<Reverse<(Score, String)>> = BinaryHeap::with_capacity(want + 1);
    // 同一 chunk 的多段只保留分数最高的那段（否则长块会霸占结果）。
    let mut best: HashMap<String, Hit> = HashMap::new();
    for row in rows {
        let (chunk_id, part, blob, segment_text, char_start, char_end) = row?;
        let score = cosine_similarity(query_vec, &deserialize_embedding(&blob));
        let hit = Hit {
            score,
            part: part.max(0) as usize,
            segment_text,
            char_span: match (char_start, char_end) {
                (Some(a), Some(b)) if a >= 0 && b >= 0 => Some((a as usize, b as usize)),
                _ => None,
            },
        };
        if best.get(&chunk_id).map(|p| hit.score <= p.score).unwrap_or(false) {
            continue;
        }
        best.insert(chunk_id.clone(), hit);
        heap.push(Reverse((Score(score), chunk_id)));
        if heap.len() > want {
            heap.pop();
        }
    }

    let mut picked: Vec<(f32, String)> = heap
        .into_iter()
        .map(|Reverse((score, id))| (score.0, id))
        .collect();
    // 堆本身无序，按分数降序输出，保证调用方看到的结果稳定可预期。
    picked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // ---- 阶段二：段文本已在手；未切分的段才回查整块正文 ----
    let need_body: Vec<&String> = picked
        .iter()
        .filter(|(_, id)| best.get(id).map(|h| h.segment_text.is_none()).unwrap_or(true))
        .map(|(_, id)| id)
        .collect();
    let mut bodies: HashMap<String, String> = HashMap::new();
    for group in need_body.chunks(ID_BATCH) {
        if group.is_empty() {
            continue;
        }
        let placeholders = vec!["?"; group.len()].join(",");
        let sql = format!("SELECT id, content FROM chunks WHERE id IN ({placeholders})");
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map(rusqlite::params_from_iter(group.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in mapped {
            let (id, content) = row?;
            bodies.insert(id, content);
        }
    }

    // 去重后截到 top_k；chunk 已不存在的条目跳过（旧 INNER JOIN 语义）。
    let mut out: Vec<RetrievedPassage> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (score, id) in picked {
        if out.len() >= top_k {
            break;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(hit) = best.get(&id) else { continue };
        // 诚实性：只返回**真正参与嵌入**的那段文本，不拿整个 chunk 冒充证据。
        let content = match (&hit.segment_text, bodies.get(&id)) {
            (Some(seg), _) => seg.clone(),
            (None, Some(full)) => full.clone(),
            (None, None) => continue,
        };
        out.push(RetrievedPassage {
            kind: ContextKind::Chunk,
            id,
            title: "(chunk)".into(),
            content,
            score,
            source_id: None,
            part: hit.part,
            char_span: hit.char_span,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    const MODEL: &str = "text-embedding-3-small";

    fn seed(conn: &Connection, id: &str, content: &str) {
        // content_hash 上有 UNIQUE 约束，用 id 派生以保证每行唯一。
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES (?1,'t',?2,?3)",
            params![id, content, format!("h-{id}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks(id, document_id, chunk_index, start_offset, end_offset, content, char_count) \
             VALUES (?1,?1,0,0,?2,?2,?2)",
            params![id, content],
        )
        .unwrap();
    }

    #[test]
    fn embedding_roundtrip_and_nearest_chunk() {
        let conn = memory_db();
        seed(&conn, "c1", "hello world");

        let emb = vec![1.0f32, 2.0, 3.0];
        store_embedding(&conn, "c1", MODEL, &emb, 0, None).unwrap();

        let loaded = load_embedding(&conn, "c1").unwrap().unwrap();
        assert_eq!(loaded, emb, "embedding 应原样往返");

        let missing = load_embedding(&conn, "nope").unwrap();
        assert!(missing.is_none(), "不存在的 chunk 应返回 None");

        let nearest = nearest_chunks(&conn, MODEL, &emb, 5).unwrap();
        assert_eq!(nearest.len(), 1);
        assert_eq!(nearest[0].id, "c1");
        assert_eq!(nearest[0].content, "hello world");
        assert!(
            (nearest[0].score - 1.0).abs() < 1e-6,
            "相同向量余弦应为 1.0"
        );
    }

    /// PERF-01：只列出「当前模型下缺向量」的 chunk。
    #[test]
    fn missing_embedding_lists_only_what_needs_computing() {
        let conn = memory_db();
        seed(&conn, "c1", "alpha");
        seed(&conn, "c2", "beta");
        seed(&conn, "c3", "gamma");

        // 全部缺失
        let all = chunks_missing_embedding(&conn, MODEL).unwrap();
        assert_eq!(all.len(), 3, "没有任何向量时应全部待算");

        // 算完 c1
        store_embedding(&conn, "c1", MODEL, &[1.0, 0.0], 0, None).unwrap();
        let rest = chunks_missing_embedding(&conn, MODEL).unwrap();
        assert_eq!(rest.len(), 2, "已有向量的 c1 不应再出现");
        assert!(rest.iter().all(|(id, _)| id != "c1"));

        // 换模型 → 全部重算（每 chunk 只有一行向量，PK 覆盖）
        let switched = chunks_missing_embedding(&conn, "text-embedding-3-large").unwrap();
        assert_eq!(switched.len(), 3, "切换模型后应全部重新向量化");
    }

    /// PERF-02：检索必须按 model 过滤（此前没过滤，导致索引用不上、且可能跨模型误比）。
    #[test]
    fn nearest_chunks_filters_by_model() {
        let conn = memory_db();
        seed(&conn, "c1", "alpha");
        store_embedding(&conn, "c1", MODEL, &[1.0, 0.0], 0, None).unwrap();

        let other = nearest_chunks(&conn, "some-other-model", &[1.0, 0.0], 5).unwrap();
        assert!(other.is_empty(), "模型不匹配时不应返回任何条目");
    }

    /// PERF-02：有界堆的 top_k 结果必须与「全量排序后取前 K」完全一致。
    #[test]
    fn bounded_heap_matches_full_sort_reference() {
        let conn = memory_db();
        // 固定种子生成 200 个 chunk 与随机向量（确定性可复现）。
        let mut seed_state = 20240917u64;
        let mut next = || {
            seed_state = seed_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed_state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
        };
        let dim = 8usize;
        for i in 0..200 {
            let id = format!("c{i:03}");
            seed(&conn, &id, "x");
            let v: Vec<f32> = (0..dim).map(|_| next()).collect();
            store_embedding(&conn, &id, MODEL, &v, 0, None).unwrap();
        }
        let q: Vec<f32> = (0..dim).map(|_| next()).collect();

        // 参考实现：全量打分 + 全量排序
        let mut reference: Vec<(f32, String)> = Vec::new();
        {
            let mut stmt = conn
                .prepare("SELECT chunk_id, embedding FROM chunk_embeddings WHERE model = ?1")
                .unwrap();
            let rows = stmt
                .query_map(params![MODEL], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
                })
                .unwrap();
            for row in rows {
                let (id, blob) = row.unwrap();
                reference.push((cosine_similarity(&q, &deserialize_embedding(&blob)), id));
            }
        }
        reference.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        for k in [1usize, 15, 200] {
            let got = nearest_chunks(&conn, MODEL, &q, k).unwrap();
            let expected: Vec<String> =
                reference.iter().take(k).map(|(_, id)| id.clone()).collect();
            let actual: Vec<String> = got.iter().map(|p| p.id.clone()).collect();
            assert_eq!(actual, expected, "k={k} 时 top-k 必须与全量排序一致");
        }
    }

    /// PERF-02：top_k=0 与「所有 chunk 行都存在」等边界不应 panic。
    #[test]
    fn nearest_chunks_handles_zero_k() {
        let conn = memory_db();
        assert!(nearest_chunks(&conn, MODEL, &[1.0, 0.0], 0).unwrap().is_empty());
    }
}

#[cfg(test)]
mod segment_tests {
    use super::*;
    use crate::ai::segmentation::split_for_embedding;
    use crate::infrastructure::db;

    const MODEL: &str = "bge-small-zh-v1.5";

    fn setup() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES ('d1','t','body','h1')",
            [],
        )
        .unwrap();
        conn
    }

    fn add_chunk(conn: &Connection, id: &str, content: &str) {
        conn.execute(
            "INSERT INTO chunks(id, document_id, chunk_index, start_offset, end_offset, content, char_count)
             VALUES (?1,'d1',0,0,?2,?2,?2)",
            params![id, content],
        )
        .unwrap();
    }

    /// 长 chunk 切成多段后，**每一段都要有向量**——否则尾部内容仍然搜不到。
    #[test]
    fn long_chunk_gets_one_vector_per_segment() {
        let conn = setup();
        let long = "知识契约与演化规范".repeat(200); // 2000+ 字，远超 510 token
        add_chunk(&conn, "c1", &long);
        let segs = split_for_embedding(&long, 510, 64);
        assert!(segs.len() > 1, "长 chunk 应被切成多段");
        for (part, seg) in segs.iter().enumerate() {
            let v = vec![1.0f32, 0.0, 0.0];
            store_embedding(&conn, "c1", MODEL, &v, part, Some(seg)).unwrap();
        }
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunk_embeddings WHERE chunk_id='c1' AND model=?1",
                params![MODEL],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows as usize, segs.len(), "每段都应各有一行向量");
    }

    /// 诚实性核心断言：命中的**长 chunk 只返回那一段的文本**，不返回整块。
    #[test]
    fn retrieval_returns_the_matched_segment_not_the_whole_chunk() {
        let conn = setup();
        let long = "甲".repeat(600); // 段0
        let tail = "乙".repeat(600); // 段1
        let full = format!("{long}{tail}");
        add_chunk(&conn, "c1", &full);
        let segs = split_for_embedding(&full, 510, 64);
        assert!(segs.len() >= 2, "需要至少两段");

        // 只给第 2 段打上与 query 完全一致的向量
        let target = segs.len() - 1;
        store_embedding(&conn, "c1", MODEL, &[1.0, 0.0, 0.0], target, Some(&segs[target])).unwrap();

        let hits = nearest_chunks(&conn, MODEL, &[1.0, 0.0, 0.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.part, target, "应报告命中的是第几段");
        assert_eq!(hit.content, segs[target].text, "内容必须**就是**参与嵌入的那一段");
        assert!(
            !hit.content.contains('甲'),
            "未参与嵌入的前段内容不得出现在证据里（虚报证据范围）"
        );
    }

    /// 多模型共存：同一 chunk 可同时持有不同模型的向量，切换不再全库重算。
    #[test]
    fn multiple_models_coexist_for_the_same_chunk() {
        let conn = setup();
        add_chunk(&conn, "c1", "短文本");
        store_embedding(&conn, "c1", "model-a", &[1.0, 0.0], 0, None).unwrap();
        store_embedding(&conn, "c1", "model-b", &[0.0, 1.0], 0, None).unwrap();
        let a = nearest_chunks(&conn, "model-a", &[1.0, 0.0], 5).unwrap();
        let b = nearest_chunks(&conn, "model-b", &[0.0, 1.0], 5).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert!((a[0].score - 1.0).abs() < 1e-6, "model-a 应命中自己");
        assert!((b[0].score - 1.0).abs() < 1e-6, "model-b 应命中自己");
    }

    /// 同一 chunk 多段都接近命中时，只返回分数最高的那一段（不霸占结果）。
    #[test]
    fn multiple_segments_of_same_chunk_are_deduped() {
        let conn = setup();
        let full = "丙".repeat(1200);
        add_chunk(&conn, "c1", &full);
        let segs = split_for_embedding(&full, 510, 64);
        assert!(segs.len() >= 2);
        for (part, seg) in segs.iter().enumerate() {
            // 第 0 段给最高分，其余给较低分
            let v = if part == 0 { vec![1.0, 0.0] } else { vec![0.9, 0.1] };
            store_embedding(&conn, "c1", MODEL, &v, part, Some(seg)).unwrap();
        }
        let hits = nearest_chunks(&conn, MODEL, &[1.0, 0.0], 5).unwrap();
        let same: Vec<&str> = hits.iter().filter(|h| h.id == "c1").map(|h| h.id.as_str()).collect();
        assert_eq!(same.len(), 1, "同一 chunk 只应出现一次");
        assert_eq!(hits[0].part, 0, "应保留分数最高的第 0 段");
    }

    /// 短 chunk 行为与改造前完全等价（part=0、无 segment_text、回查整块）。
    #[test]
    fn short_chunk_behaves_exactly_as_before() {
        let conn = setup();
        add_chunk(&conn, "c1", "短文本");
        store_embedding(&conn, "c1", MODEL, &[1.0, 0.0], 0, None).unwrap();
        let hits = nearest_chunks(&conn, MODEL, &[1.0, 0.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].part, 0);
        assert_eq!(hits[0].char_span, None);
        assert_eq!(hits[0].content, "短文本", "未切分时应返回整块正文");
    }
}
