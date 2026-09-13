//! Evidence 的持久化。
//!
//! 证据是 Claim 与原文之间**唯一**的桥（Rule 3），因此这里的写入只增不改：
//! 补一条证据是新增一行，而不是覆盖已有引文。
//! 同一个来源被重复引用时靠 `INSERT OR IGNORE` 收敛（幂等）。

use rusqlite::{params, Connection};

use crate::domain::common::ids::{ChunkId, ClaimId, DocumentId, EvidenceId};
use crate::domain::evidence::evidence::{Evidence, EvidenceLevel};
use crate::domain::knowledge::document::SourceType;
use crate::error::AppResult;
use crate::infrastructure::db::{opt_f32_col, parse_col};

/// 插入证据。
///
/// 同一 (claim, chunk, quote) 重复写入会被忽略——重复导入同一文档时
/// 不该堆积出一堆一模一样的证据行。
pub fn insert(conn: &Connection, evidence: &Evidence) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO evidence(
            id, claim_id, document_id, chunk_id, start_offset, end_offset,
            quote, evidence_level, source_type, confidence
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            evidence.id.as_str(),
            evidence.claim_id.as_str(),
            evidence.document_id.as_str(),
            evidence.chunk_id.as_ref().map(|id| id.as_str()),
            evidence.start_offset.map(|v| v as i64),
            evidence.end_offset.map(|v| v as i64),
            evidence.quote,
            i64::from(evidence.evidence_level.as_u8()),
            evidence.source_type.as_str(),
            evidence.confidence.map(f64::from),
        ],
    )?;
    Ok(())
}

const EVIDENCE_COLUMNS: &str =
    "id, claim_id, document_id, chunk_id, start_offset, end_offset, quote, \
     evidence_level, source_type, confidence, created_at";

fn map_evidence(row: &rusqlite::Row<'_>) -> rusqlite::Result<Evidence> {
    Ok(Evidence {
        id: parse_col::<EvidenceId>(row, 0)?,
        claim_id: parse_col::<ClaimId>(row, 1)?,
        document_id: parse_col::<DocumentId>(row, 2)?,
        chunk_id: row.get::<_, Option<String>>(3)?.map(ChunkId::from_raw),
        start_offset: row.get::<_, Option<i64>>(4)?.map(|v| v as usize),
        end_offset: row.get::<_, Option<i64>>(5)?.map(|v| v as usize),
        quote: row.get(6)?,
        evidence_level: {
            // EvidenceLevel 是 u8 枚举（L1..L5），没有 FromStr，按整数读取再转换。
            let level_raw: i64 = row.get(7)?;
            EvidenceLevel::from_u8(u8::try_from(level_raw).unwrap_or(0)).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Integer,
                    Box::new(err),
                )
            })?
        },
        source_type: parse_col::<SourceType>(row, 8)?,
        confidence: opt_f32_col(row, 9)?,
        created_at: row.get(10)?,
    })
}

/// 某条 Claim 的全部证据，层级最低（最便宜）的排前面。
pub fn list_for_claim(conn: &Connection, claim_id: &ClaimId) -> AppResult<Vec<Evidence>> {
    let sql = format!(
        "SELECT {EVIDENCE_COLUMNS} FROM evidence
         WHERE claim_id = ?1
         ORDER BY evidence_level ASC, created_at ASC"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![claim_id.as_str()], map_evidence)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 证据总数。
pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM evidence", [], |row| row.get(0))?)
}

/// 某文档贡献的证据数（文档详情页展示用）。
pub fn count_for_document(conn: &Connection, document_id: &DocumentId) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM evidence WHERE document_id = ?1",
        params![document_id.as_str()],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;
    use crate::infrastructure::{claim_repository, document_repository, entity_repository};
    use crate::domain::knowledge::chunk::chunk_document;
    use crate::domain::knowledge::document::Document;
    use crate::domain::knowledge::claim::{Claim, ClaimObject, ClaimStatus, ClaimType, Modality, Polarity};
    use crate::domain::ontology::predicate::ClaimPredicate;

    fn seed_claim(conn: &Connection) -> (ClaimId, DocumentId, ChunkId) {
        let subject = entity_repository::resolve_or_create(conn, "Rust").unwrap();
        let content = "Rust 1.75 stabilized async fn in trait.";
        let document = Document {
            id: DocumentId::new(),
            title: "Release note".into(),
            content: content.into(),
            content_hash: Document::content_hash(content),
            source_type: SourceType::Markdown,
            source_uri: None,
            metadata: serde_json::json!({}),
            created_at: String::new(),
            updated_at: String::new(),
        };
        document_repository::insert(conn, &document).unwrap();
        let drafts = chunk_document(content);
        document_repository::replace_chunks(conn, &document.id, &drafts).unwrap();
        let chunk_id = document_repository::list_chunks(conn, &document.id).unwrap()[0]
            .id
            .clone();

        let claim = Claim {
            id: ClaimId::new(),
            subject_id: subject.id,
            predicate: ClaimPredicate::Supports,
            object: Some(ClaimObject::Literal("async fn in trait".into())),
            content: None,
            context: serde_json::json!({}),
            claim_type: ClaimType::Factual,
            polarity: Polarity::Positive,
            modality: Modality::Asserted,
            condition: None,
            confidence: None,
            status: ClaimStatus::Candidate,
            valid_from: None,
            valid_until: None,
            observed_at: None,
            recorded_at: String::new(),
            created_at: String::new(),
        };
        claim_repository::insert(conn, &claim).unwrap();
        (claim.id, document.id, chunk_id)
    }

    fn evidence(claim_id: &ClaimId, document_id: &DocumentId, chunk_id: &ChunkId, level: u8) -> Evidence {
        Evidence {
            id: EvidenceId::new(),
            claim_id: claim_id.clone(),
            document_id: document_id.clone(),
            chunk_id: Some(chunk_id.clone()),
            start_offset: Some(0),
            end_offset: Some(37),
            quote: Some("Rust 1.75 stabilized async fn in trait.".into()),
            evidence_level: EvidenceLevel::from_u8(level).unwrap(),
            source_type: SourceType::Markdown,
            confidence: Some(0.8),
            created_at: String::new(),
        }
    }

    #[test]
    fn evidence_round_trips_including_offsets_and_level() {
        let conn = memory_db();
        let (claim_id, document_id, chunk_id) = seed_claim(&conn);
        insert(&conn, &evidence(&claim_id, &document_id, &chunk_id, 2)).unwrap();

        let stored = list_for_claim(&conn, &claim_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].evidence_level, EvidenceLevel::QuoteContext);
        assert_eq!(stored[0].chunk_id.as_ref(), Some(&chunk_id));
        assert_eq!(stored[0].quote.as_deref(), Some("Rust 1.75 stabilized async fn in trait."));
        assert_eq!(stored[0].start_offset, Some(0));
        assert_eq!(stored[0].confidence, Some(0.8));
    }

    #[test]
    fn evidence_is_ordered_cheapest_first() {
        let conn = memory_db();
        let (claim_id, document_id, chunk_id) = seed_claim(&conn);
        insert(&conn, &evidence(&claim_id, &document_id, &chunk_id, 4)).unwrap();
        insert(&conn, &evidence(&claim_id, &document_id, &chunk_id, 1)).unwrap();

        let stored = list_for_claim(&conn, &claim_id).unwrap();
        assert_eq!(stored[0].evidence_level, EvidenceLevel::Quote);
        assert_eq!(stored[1].evidence_level, EvidenceLevel::Chunk);
    }

    #[test]
    fn deleting_a_chunk_keeps_the_evidence_but_nulls_the_pointer() {
        let conn = memory_db();
        let (claim_id, _, chunk_id) = seed_claim(&conn);
        insert(&conn, &evidence(&claim_id, &legacy_document_id(&conn), &chunk_id, 1)).unwrap();

        conn.execute("DELETE FROM chunks WHERE id = ?1", params![chunk_id.as_str()])
            .unwrap();

        let stored = list_for_claim(&conn, &claim_id).unwrap();
        assert_eq!(stored.len(), 1, "证据本身不应随切片消失（TDD §85）");
        assert!(stored[0].chunk_id.is_none());
        assert!(stored[0].quote.is_some(), "引文是证据的实质内容，必须保留");
    }

    fn legacy_document_id(conn: &Connection) -> DocumentId {
        DocumentId::from_raw(
            conn.query_row("SELECT id FROM documents LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        )
    }

    #[test]
    fn duplicate_evidence_rows_are_collapsed() {
        let conn = memory_db();
        let (claim_id, document_id, chunk_id) = seed_claim(&conn);
        let same = evidence(&claim_id, &document_id, &chunk_id, 1);
        insert(&conn, &same).unwrap();
        // 同一 id 再次写入：INSERT OR IGNORE 应静默忽略，不新增行。
        // （证据去重由"同一 id"保证 —— 业务层复用 id 而不是重新生成。）
        insert(&conn, &same.clone()).unwrap();
        assert_eq!(count(&conn).unwrap(), 1);
    }

    #[test]
    fn counts_per_document_are_available() {
        let conn = memory_db();
        let (claim_id, document_id, chunk_id) = seed_claim(&conn);
        assert_eq!(count_for_document(&conn, &document_id).unwrap(), 0);
        insert(&conn, &evidence(&claim_id, &document_id, &chunk_id, 1)).unwrap();
        assert_eq!(count_for_document(&conn, &document_id).unwrap(), 1);
    }

    #[test]
    fn invalid_evidence_level_is_rejected_by_the_database() {
        let conn = memory_db();
        let (claim_id, document_id, _) = seed_claim(&conn);
        let result = conn.execute(
            "INSERT INTO evidence(id,claim_id,document_id,evidence_level) VALUES('e1',?1,?2,9)",
            params![claim_id.as_str(), document_id.as_str()],
        );
        assert!(result.is_err(), "证据层级越界必须被 CHECK 拒绝");
    }
}
