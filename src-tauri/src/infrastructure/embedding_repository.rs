//! 语义检索的向量存储（Phase 6）。
//!
//! 设计要点：
//! - `chunk_embeddings` 是派生数据（0002 迁移），chunk 删除即失效，重算即可恢复。
//! - embedding 以 little-endian f32 序列化成 `BLOB` 落库，读取时反向还原。
//! - `nearest_chunks` 做暴力余弦相似度（库规模小，无需 ANN 索引），诚实返回真实分数；
//!   无任何伪造/兜底分数（TDD「诚实优先」）。
//! - 所有函数失败都通过 `AppResult` 上抛；调用方（semantic_search）决定如何降级。

use rusqlite::{params, Connection, OptionalExtension};

use crate::ai::context::ContextKind;
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

/// 存储（或覆盖）单个 chunk 的 embedding。
///
/// `chunk_id` 是 PRIMARY KEY，重复写入即覆盖——这正是 semantic_search
/// 在每次查询时「按需重算并覆盖」所依赖的语义。
pub fn store_embedding(
    conn: &Connection,
    chunk_id: &str,
    embedding: &[f32],
    model: &str,
) -> AppResult<()> {
    let blob = serialize_embedding(embedding);
    let dimensions = embedding.len() as i64;
    conn.execute(
        "INSERT INTO chunk_embeddings (chunk_id, embedding, dimensions, model, created_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))
         ON CONFLICT(chunk_id) DO UPDATE SET
           embedding = excluded.embedding,
           dimensions = excluded.dimensions,
           model = excluded.model,
           created_at = datetime('now')",
        params![chunk_id, blob, dimensions, model],
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

/// 读取全部 chunk 文本：(chunk_id, content)，供向量化与结果回填。
pub fn all_chunk_texts(conn: &Connection) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, content FROM chunks")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
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

/// 对库内所有 chunk embedding 做暴力余弦近邻，返回 top_k。
///
/// content 直接 JOIN `chunks` 取回原文，避免二次查询。
/// 分数即真实余弦相似度（[-1,1] 区间），绝不伪造。
pub fn nearest_chunks(
    conn: &Connection,
    query_vec: &[f32],
    top_k: usize,
) -> AppResult<Vec<RetrievedPassage>> {
    let mut stmt = conn.prepare(
        "SELECT ce.chunk_id, ce.embedding, c.content
         FROM chunk_embeddings ce
         JOIN chunks c ON c.id = ce.chunk_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut scored: Vec<(f32, RetrievedPassage)> = Vec::new();
    for row in rows {
        let (chunk_id, blob, content) = row?;
        let vec = deserialize_embedding(&blob);
        let score = cosine_similarity(query_vec, &vec);
        scored.push((
            score,
            RetrievedPassage {
                kind: ContextKind::Chunk,
                id: chunk_id,
                title: "(chunk)".into(),
                content,
                score,
                source_id: None,
            },
        ));
    }

    // 按相似度降序，取前 top_k。
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored
        .into_iter()
        .take(top_k)
        .map(|(_, passage)| passage)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    #[test]
    fn embedding_roundtrip_and_nearest_chunk() {
        let conn = memory_db();
        // chunks 外键依赖 documents，先建文档再建切片。
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES ('d1','t','hello world','h1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks(id, document_id, chunk_index, start_offset, end_offset, content, char_count) \
             VALUES ('c1','d1',0,0,11,'hello world',11)",
            [],
        )
        .unwrap();

        let emb = vec![1.0f32, 2.0, 3.0];
        store_embedding(&conn, "c1", &emb, "text-embedding-3-small").unwrap();

        let loaded = load_embedding(&conn, "c1").unwrap().unwrap();
        assert_eq!(loaded, emb, "embedding 应原样往返");

        let missing = load_embedding(&conn, "nope").unwrap();
        assert!(missing.is_none(), "不存在的 chunk 应返回 None");

        let nearest = nearest_chunks(&conn, &emb, 5).unwrap();
        assert_eq!(nearest.len(), 1);
        assert_eq!(nearest[0].id, "c1");
        assert_eq!(nearest[0].content, "hello world");
        assert!((nearest[0].score - 1.0).abs() < 1e-6, "相同向量余弦应为 1.0");
    }
}
