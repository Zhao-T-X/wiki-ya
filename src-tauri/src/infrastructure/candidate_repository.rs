//! `candidates` 候选知识仓储（M6）。
//!
//! 候选一产生就持久化；决策（accept/reject）只改状态，永不物理删除——
//! 拒绝也是留痕（Trace 可回答「为什么这条没进知识库」）。

use rusqlite::{Connection, OptionalExtension};

use crate::domain::knowledge::candidate::{Candidate, CandidateStatus};
use crate::error::AppResult;
use crate::infrastructure::db::{now, parse_col};

const COLS: &str = "id, run_id, document_id, subject, predicate, object_text, content, \
                     claim_type, polarity, modality, confidence, source_chunk_index, \
                     source_quote, sentence, status, accepted_claim_id, reject_reason, created_at";

/// 插入一条候选（created_at 由仓储生成）。
pub fn insert(conn: &Connection, candidate: &Candidate) -> AppResult<()> {
    let created_at = now(conn)?;
    conn.execute(
        "INSERT INTO candidates(id, run_id, document_id, subject, predicate, object_text, \
         content, claim_type, polarity, modality, confidence, source_chunk_index, \
         source_quote, sentence, status, reject_reason, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
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

/// 读取单条候选。
pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Candidate>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM candidates WHERE id = ?1"
    ))?;
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
        status: parse_col::<CandidateStatus>(r, 14)?,
        accepted_claim_id: r.get(15)?,
        reject_reason: r.get(16)?,
        created_at: r.get(17)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        decide(&conn, "c1", CandidateStatus::Accepted, Some("claim-9"), None).unwrap();
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
        let c2 = get(&conn, "c2").unwrap().unwrap();
        assert_eq!(c2.status, CandidateStatus::Rejected);
        assert_eq!(c2.reject_reason.as_deref(), Some("与已有知识重复"));
    }
}
