//! `candidates` 候选知识仓储（M6）。
//!
//! 候选一产生就持久化；决策（accept/reject）只改状态，永不物理删除——
//! 拒绝也是留痕（Trace 可回答「为什么这条没进知识库」）。

use rusqlite::{Connection, OptionalExtension};

use crate::domain::knowledge::candidate::{Candidate, CandidateStatus, SupportLevel};
use crate::error::AppResult;
use crate::infrastructure::db::{now, parse_col};

const COLS: &str = "id, run_id, document_id, subject, predicate, object_text, content, \
                     claim_type, polarity, modality, confidence, source_chunk_index, \
                     source_quote, sentence, support_level, status, accepted_claim_id, \
                     reject_reason, created_at";

/// 插入一条候选（created_at 由仓储生成）。
pub fn insert(conn: &Connection, candidate: &Candidate) -> AppResult<()> {
    let created_at = now(conn)?;
    conn.execute(
        "INSERT INTO candidates(id, run_id, document_id, subject, predicate, object_text, \
         content, claim_type, polarity, modality, confidence, source_chunk_index, \
         source_quote, sentence, support_level, status, reject_reason, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        rusqlite::params![
            candidate.id,
            candidate.run_id,
            candidate.document_id,
            candidate.subject,
            candidate.predicate,
            candidate.object_text,
            candidate.content,
            candidate.claim_type,
            candidate.polarity,
            candidate.modality,
            candidate.confidence,
            candidate.source_chunk_index,
            candidate.source_quote,
            candidate.sentence,
            candidate.support_level.as_str(),
            candidate.status.as_str(),
            candidate.reject_reason,
            created_at
        ],
    )?;
    Ok(())
}

/// 按 Run 列出候选（产生顺序）。
pub fn list_by_run(conn: &Connection, run_id: &str) -> AppResult<Vec<Candidate>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM candidates WHERE run_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map(rusqlite::params![run_id], map_candidate)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 候选列表的游标（PERF-04）：不透明串，编码 `created_at|id`。
///
/// 用游标而非 `OFFSET`：候选会随抽取持续增长，OFFSET 会随偏移量线性退化
/// （翻到第 k 页要扫过前 k 页），且并发插入时 OFFSET 会漏行/重复行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateCursor {
    pub created_at: String,
    pub id: String,
}

impl CandidateCursor {
    /// 解析不透明游标串；格式非法时返回 `None`（当作"从头开始"，不报错）。
    pub fn parse(raw: &str) -> Option<Self> {
        // created_at 形如 "2026-01-01 12:00:00"，本身不含 '|'，故取最后一次分隔。
        let (created_at, id) = raw.rsplit_once('|')?;
        if created_at.is_empty() || id.is_empty() {
            return None;
        }
        Some(CandidateCursor {
            created_at: created_at.to_string(),
            id: id.to_string(),
        })
    }

    pub fn encode(&self) -> String {
        format!("{}|{}", self.created_at, self.id)
    }
}

/// 游标分页取候选（PERF-04）。
///
/// 走 `idx_candidates_run_cursor(run_id, created_at, id)`：
/// `WHERE run_id = ?` + 复合游标 + `ORDER BY created_at, id` 完全契合该索引，
/// 数据库层无需排序、也无需跳过前 k 页。
pub fn list_by_run_page(
    conn: &Connection,
    run_id: &str,
    cursor: Option<&CandidateCursor>,
    limit: usize,
) -> AppResult<Vec<Candidate>> {
    let limit = limit.clamp(1, 500) as i64;
    let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match cursor {
        Some(c) => (
            format!(
                "SELECT {COLS} FROM candidates
                 WHERE run_id = ?1 AND (created_at, id) > (?2, ?3)
                 ORDER BY created_at, id
                 LIMIT ?4"
            ),
            vec![
                Box::new(run_id.to_string()),
                Box::new(c.created_at.clone()),
                Box::new(c.id.clone()),
                Box::new(limit),
            ],
        ),
        None => (
            format!(
                "SELECT {COLS} FROM candidates
                 WHERE run_id = ?1
                 ORDER BY created_at, id
                 LIMIT ?2"
            ),
            vec![Box::new(run_id.to_string()), Box::new(limit)],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_candidate)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 读取单条候选。
pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Candidate>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM candidates WHERE id = ?1"))?;
    let row = stmt
        .query_row(rusqlite::params![id], map_candidate)
        .optional()?;
    Ok(row)
}

/// 记录用户决策：accept（附 Claim id）或 reject（附原因）。
pub fn decide(
    conn: &Connection,
    id: &str,
    status: CandidateStatus,
    accepted_claim_id: Option<&str>,
    reject_reason: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE candidates SET status = ?2, accepted_claim_id = ?3, reject_reason = ?4 \
         WHERE id = ?1",
        rusqlite::params![id, status.as_str(), accepted_claim_id, reject_reason],
    )?;
    Ok(())
}

fn map_candidate(r: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        id: r.get(0)?,
        run_id: r.get(1)?,
        document_id: r.get(2)?,
        subject: r.get(3)?,
        predicate: r.get(4)?,
        object_text: r.get(5)?,
        content: r.get(6)?,
        claim_type: r.get(7)?,
        polarity: r.get(8)?,
        modality: r.get(9)?,
        confidence: r.get::<_, Option<f64>>(10)?.map(|v| v as f32),
        source_chunk_index: r.get(11)?,
        source_quote: r.get(12)?,
        sentence: r.get(13)?,
        support_level: parse_col::<SupportLevel>(r, 14)?,
        status: parse_col::<CandidateStatus>(r, 15)?,
        accepted_claim_id: r.get(16)?,
        reject_reason: r.get(17)?,
        created_at: r.get(18)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::candidate::{Candidate, CandidateStatus, SupportLevel};
    use crate::infrastructure::db;

    fn sample(id: &str, run_id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            run_id: run_id.into(),
            document_id: "doc-1".into(),
            subject: "Rust".into(),
            predicate: "enables".into(),
            object_text: Some("安全并发".into()),
            content: None,
            claim_type: Some("factual".into()),
            polarity: None,
            modality: None,
            confidence: Some(0.9),
            source_chunk_index: Some(3),
            source_quote: Some("Rust enables 安全并发".into()),
            sentence: Some("Rust enables 安全并发。".into()),
            support_level: SupportLevel::Partially,
            status: CandidateStatus::Pending,
            accepted_claim_id: None,
            reject_reason: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn insert_list_decide_round_trip() {
        let mut conn = Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();

        // 父行：documents / extraction_runs（外键约束）。
        conn.execute(
            "INSERT INTO documents(id, title, content, content_hash) \
             VALUES ('doc-1', '测试文档', '正文', 'hash-1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO extraction_runs(id, document_id, status, stage, started_at) \
             VALUES ('r1', 'doc-1', 'queued', 'preparing', datetime('now'))",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO entities(id, name, primary_type) VALUES ('e1', 'Rust', 'concept')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claims(id, subject_id, predicate) VALUES ('claim-9', 'e1', 'enables')",
            [],
        )
        .unwrap();
        insert(&conn, &sample("c1", "r1")).unwrap();
        insert(&conn, &sample("c2", "r1")).unwrap();

        let all = list_by_run(&conn, "r1").unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|c| c.status == CandidateStatus::Pending));

        // accept → 记录 claim id；reject → 记录原因。
        decide(
            &conn,
            "c1",
            CandidateStatus::Accepted,
            Some("claim-9"),
            None,
        )
        .unwrap();
        decide(
            &conn,
            "c2",
            CandidateStatus::Rejected,
            None,
            Some("与已有知识重复"),
        )
        .unwrap();

        let c1 = get(&conn, "c1").unwrap().unwrap();
        assert_eq!(c1.status, CandidateStatus::Accepted);
        assert_eq!(c1.accepted_claim_id.as_deref(), Some("claim-9"));
        assert_eq!(c1.support_level, SupportLevel::Partially);
        let c2 = get(&conn, "c2").unwrap().unwrap();
        assert_eq!(c2.status, CandidateStatus::Rejected);
        assert_eq!(c2.reject_reason.as_deref(), Some("与已有知识重复"));
    }
}

#[cfg(test)]
mod paging_tests {
    use super::*;
    use crate::infrastructure::db;

    fn setup_with(n: usize) -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO documents(id, title, content, content_hash) VALUES ('doc-1','t','body','h1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO extraction_runs(id, document_id, status, stage, started_at) \
             VALUES ('r1','doc-1','queued','preparing', datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entities(id, name, primary_type) VALUES ('e1','Rust','concept')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claims(id, subject_id, predicate) VALUES ('claim-1','e1','enables')",
            [],
        )
        .unwrap();
        for i in 0..n {
            conn.execute(
                "INSERT INTO candidates(id, run_id, document_id, subject, predicate, status, created_at) \
                 VALUES (?1,'r1','doc-1','S','p','pending', ?2)",
                rusqlite::params![format!("c{i:03}"), format!("2026-01-01 00:00:{:02}", i)],
            )
            .unwrap();
        }
        conn
    }

    fn sample(id: &str, support: SupportLevel) -> Candidate {
        Candidate {
            id: id.into(),
            run_id: "r1".into(),
            document_id: "doc-1".into(),
            subject: "S".into(),
            predicate: "p".into(),
            object_text: None,
            content: None,
            claim_type: None,
            polarity: None,
            modality: None,
            confidence: None,
            source_chunk_index: None,
            source_quote: None,
            sentence: None,
            support_level: support,
            status: CandidateStatus::Pending,
            accepted_claim_id: None,
            reject_reason: None,
            created_at: String::new(),
        }
    }

    /// PERF-04：游标分页必须不重不漏地走完所有行，且顺序稳定。
    #[test]
    fn cursor_pagination_walks_every_row_once() {
        let conn = setup_with(7);

        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<CandidateCursor> = None;
        loop {
            let page = list_by_run_page(&conn, "r1", cursor.as_ref(), 3).unwrap();
            if page.is_empty() {
                break;
            }
            for c in &page {
                assert!(
                    !seen.contains(&c.id),
                    "游标分页不应重复返回：{}",
                    c.id
                );
                seen.push(c.id.clone());
            }
            let last = page.last().unwrap();
            cursor = Some(CandidateCursor {
                created_at: last.created_at.clone(),
                id: last.id.clone(),
            });
            if page.len() < 3 {
                break;
            }
        }
        assert_eq!(seen.len(), 7, "7 条候选应恰好走完一遍");
        assert_eq!(seen.first().unwrap(), "c000");
        assert_eq!(seen.last().unwrap(), "c006");
    }

    /// 游标编解码往返；非法串当作"从头开始"而不是报错。
    #[test]
    fn cursor_codec_round_trips_and_tolerates_garbage() {
        let c = CandidateCursor {
            created_at: "2026-01-01 00:00:01".into(),
            id: "c001".into(),
        };
        assert_eq!(CandidateCursor::parse(&c.encode()).unwrap(), c);
        assert!(CandidateCursor::parse("").is_none());
        assert!(CandidateCursor::parse("no-separator").is_none());
        assert!(CandidateCursor::parse("|c1").is_none());
    }

    /// PERF-04（核心验收）：溯源按 accepted_claim_id 反查候选**必须走索引**。
    /// 这条查询此前完全没有索引——即「Claim 溯源」用得越久越慢的根因。
    #[test]
    fn claim_trace_lookup_uses_the_accepted_claim_index() {
        let conn = setup_with(2);
        let sql = "SELECT id, run_id, status, support_level, created_at FROM candidates \
                   WHERE accepted_claim_id = ?1 ORDER BY created_at DESC LIMIT 1";
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan: Vec<String> = stmt
            .query_map(rusqlite::params!["claim-1"], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let detail = format!("{plan:?}").to_lowercase();
        assert!(
            detail.contains("idx_candidates_accepted_claim"),
            "溯源查询必须命中 idx_candidates_accepted_claim，实际计划：{detail}"
        );
    }

    /// PERF-04：游标分页查询必须走复合游标索引，且**无需临时排序**。
    #[test]
    fn cursor_paging_uses_composite_index_without_temp_sort() {
        let conn = setup_with(2);
        let sql = "SELECT id FROM candidates
                   WHERE run_id = ?1 AND (created_at, id) > (?2, ?3)
                   ORDER BY created_at, id LIMIT ?4";
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan: Vec<String> = stmt
            .query_map(
                rusqlite::params!["r1", "2026-01-01 00:00:00", "c000", 50],
                |r| r.get::<_, String>(3),
            )
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let detail = format!("{plan:?}").to_lowercase();
        assert!(
            detail.contains("idx_candidates_run_cursor"),
            "分页查询应命中 idx_candidates_run_cursor，实际：{detail}"
        );
        assert!(
            !detail.contains("temp b-tree"),
            "有复合游标索引时不应再出现临时排序，实际：{detail}"
        );
    }

    /// 游标分页写入路径仍可用（覆盖 insert → 分页读取）。
    #[test]
    fn paged_read_after_insert() {
        let conn = setup_with(0);
        insert(&conn, &sample("x1", SupportLevel::Directly)).unwrap();
        insert(&conn, &sample("x2", SupportLevel::Unsupported)).unwrap();
        let page = list_by_run_page(&conn, "r1", None, 10).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].id, "x1");
        assert_eq!(page[1].id, "x2");
    }
}
