//! Claim 的持久化与候选检索。
//!
//! 两个刻意的设计：
//!
//! 1. **没有 `update_claim_content`**：Rule 2 要求 Claim 只增不改。
//!    唯一允许改的列是 `status`，而它由 `claim_relation_repository`
//!    的演化流程独占（INV-08）。
//! 2. **来源信息不在本表**：文档/引文来自 `evidence` 的 join。
//!    因此"一条 Claim 多份证据"天然成立，且不会出现两处真相。

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::common::ids::{ClaimId, DocumentId, EntityId};
use crate::domain::evolution::conflict::ClaimView;
use crate::domain::knowledge::claim::{
    Claim, ClaimObject, ClaimStatus, ClaimType, Modality, Polarity,
};
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::error::AppResult;
use crate::infrastructure::db::{json_col, opt_f32_col, parse_col};

/// Claim + 展示所需的关联信息。
#[derive(Debug, Clone)]
pub struct ClaimRow {
    pub claim: Claim,
    pub subject_name: String,
    pub object_name: Option<String>,
    pub source_document_id: Option<DocumentId>,
    pub source_document_title: Option<String>,
    pub source_quote: Option<String>,
    pub evidence_count: i64,
}

impl ClaimRow {
    /// 转成演化判定所需的视图。
    pub fn to_view(&self) -> ClaimView {
        ClaimView {
            id: self.claim.id.clone(),
            subject_id: self.claim.subject_id.clone(),
            predicate: self.claim.predicate,
            object: self.claim.object.clone(),
            polarity: self.claim.polarity,
            status: self.claim.status,
            created_at: self.claim.created_at.clone(),
        }
    }

    /// 可读陈述。抽取没给 `content` 时按"主语 谓语 宾语"拼一个，
    /// 这样 UI 永远不会出现空白标题。
    pub fn display_text(&self) -> String {
        if let Some(content) = self.claim.content.as_ref().filter(|c| !c.trim().is_empty()) {
            return content.clone();
        }
        let object = self
            .object_name
            .clone()
            .or_else(|| self.claim.object.as_ref().and_then(|o| match o {
                ClaimObject::Literal(text) => Some(text.clone()),
                ClaimObject::Number(value) => Some(value.to_string()),
                ClaimObject::Boolean(value) => Some(value.to_string()),
                ClaimObject::Date(value) => Some(value.clone()),
                ClaimObject::Entity(_) => None,
            }))
            .unwrap_or_default();
        format!("{} {} {}", self.subject_name, self.claim.predicate, object)
            .trim()
            .to_string()
    }
}

/// 主查询：把 Claim 与本征证据、主语/宾语名称一次取出。
///
/// 用窗口函数挑"最便宜的一条证据"（层级最低、最早）作为主证据：
/// 它决定了 Claim 在列表里显示的来源与引文。
const CLAIM_SELECT: &str = "
WITH primary_evidence AS (
  SELECT claim_id, document_id, quote,
         ROW_NUMBER() OVER (
           PARTITION BY claim_id ORDER BY evidence_level ASC, created_at ASC
         ) AS rn
  FROM evidence
)
SELECT
  c.id, c.subject_id, c.predicate, c.object_id, c.object_text, c.content, c.context_json,
  c.claim_type, c.polarity, c.modality, c.condition, c.confidence, c.status,
  c.valid_from, c.valid_until, c.recorded_at, c.created_at,
  s.name AS subject_name,
  o.name AS object_name,
  pe.document_id AS source_document_id,
  d.title AS source_document_title,
  pe.quote AS source_quote,
  (SELECT COUNT(*) FROM evidence e2 WHERE e2.claim_id = c.id) AS evidence_count
FROM claims c
JOIN entities s ON s.id = c.subject_id
LEFT JOIN entities o ON o.id = c.object_id
LEFT JOIN primary_evidence pe ON pe.claim_id = c.id AND pe.rn = 1
LEFT JOIN documents d ON d.id = pe.document_id
";

fn map_claim(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaimRow> {
    let object_id: Option<String> = row.get(3)?;
    let object_text: Option<String> = row.get(4)?;
    let object = match (object_id, object_text) {
        (Some(id), _) => Some(ClaimObject::Entity(EntityId::from_raw(id))),
        (None, Some(text)) if !text.trim().is_empty() => Some(ClaimObject::Literal(text)),
        _ => None,
    };

    let claim = Claim {
        id: parse_col::<ClaimId>(row, 0)?,
        subject_id: parse_col::<EntityId>(row, 1)?,
        predicate: parse_col::<ClaimPredicate>(row, 2)?,
        object,
        content: row.get(5)?,
        context: json_col(row, 6)?,
        claim_type: parse_col::<ClaimType>(row, 7)?,
        polarity: parse_col::<Polarity>(row, 8)?,
        modality: parse_col::<Modality>(row, 9)?,
        condition: row.get(10)?,
        confidence: opt_f32_col(row, 11)?,
        status: parse_col::<ClaimStatus>(row, 12)?,
        valid_from: row.get(13)?,
        valid_until: row.get(14)?,
        recorded_at: row.get(15)?,
        created_at: row.get(16)?,
    };

    Ok(ClaimRow {
        claim,
        subject_name: row.get(17)?,
        object_name: row.get(18)?,
        source_document_id: row
            .get::<_, Option<String>>(19)?
            .map(DocumentId::from_raw),
        source_document_title: row.get(20)?,
        source_quote: row.get(21)?,
        evidence_count: row.get(22)?,
    })
}

/// 插入 Claim（不改动任何已有行）。
pub fn insert(conn: &Connection, claim: &Claim) -> AppResult<()> {
    let (object_id, object_text) = match claim.object.as_ref() {
        Some(ClaimObject::Entity(id)) => (Some(id.as_str().to_string()), None),
        Some(ClaimObject::Literal(text)) => (None, Some(text.clone())),
        Some(ClaimObject::Number(value)) => (None, Some(value.to_string())),
        Some(ClaimObject::Boolean(value)) => (None, Some(value.to_string())),
        Some(ClaimObject::Date(value)) => (None, Some(value.clone())),
        None => (None, None),
    };

    conn.execute(
        "INSERT INTO claims(
            id, subject_id, predicate, object_id, object_text, content, context_json,
            claim_type, polarity, modality, condition, confidence, status,
            valid_from, valid_until
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            claim.id.as_str(),
            claim.subject_id.as_str(),
            claim.predicate.as_str(),
            object_id,
            object_text,
            claim.content,
            serde_json::to_string(&claim.context)?,
            claim.claim_type.as_str(),
            claim.polarity.as_str(),
            claim.modality.as_str(),
            claim.condition,
            claim.confidence.map(f64::from),
            claim.status.as_str(),
            claim.valid_from,
            claim.valid_until,
        ],
    )?;
    Ok(())
}

/// 按 id 取 Claim。
pub fn get(conn: &Connection, id: &ClaimId) -> AppResult<Option<ClaimRow>> {
    let sql = format!("{CLAIM_SELECT} WHERE c.id = ?1");
    Ok(conn
        .query_row(&sql, params![id.as_str()], map_claim)
        .optional()?)
}

/// Claim 列表过滤条件。全部为 `None` 表示"最近的全部"。
#[derive(Debug, Clone, Default)]
pub struct ClaimFilter {
    pub subject_id: Option<EntityId>,
    pub predicate: Option<ClaimPredicate>,
    pub status: Option<ClaimStatus>,
    /// 按来源文档过滤（通过 evidence 反查）。
    pub document_id: Option<DocumentId>,
    pub limit: usize,
}

/// 列出 Claim。
pub fn list(conn: &Connection, filter: &ClaimFilter) -> AppResult<Vec<ClaimRow>> {
    let sql = format!(
        "{CLAIM_SELECT}
         WHERE (?1 IS NULL OR c.subject_id = ?1)
           AND (?2 IS NULL OR c.predicate = ?2)
           AND (?3 IS NULL OR c.status = ?3)
           AND (?4 IS NULL OR EXISTS (
                 SELECT 1 FROM evidence e3
                 WHERE e3.claim_id = c.id AND e3.document_id = ?4))
         ORDER BY c.created_at DESC, c.id
         LIMIT ?5"
    );
    let limit = filter.limit.clamp(1, 500) as i64;
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(
        params![
            filter.subject_id.as_ref().map(|id| id.as_str()),
            filter.predicate.map(|p| p.as_str()),
            filter.status.map(|s| s.as_str()),
            filter.document_id.as_ref().map(|id| id.as_str()),
            limit,
        ],
        map_claim,
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 演化比较的候选集。
///
/// 走 `idx_claims_sp`（subject + predicate + status）：同主语同谓语才算候选。
/// 结构化召回有意**不**做语义兜底——那需要为每条 Claim 付一次嵌入调用，
/// 得先证明结构化召回的召回率不足（参考实现的判断，wiki-ya 沿用）。
pub fn find_candidates(
    conn: &Connection,
    subject_id: &EntityId,
    predicate: ClaimPredicate,
    exclude: Option<&ClaimId>,
    limit: usize,
) -> AppResult<Vec<ClaimRow>> {
    let sql = format!(
        "{CLAIM_SELECT}
         WHERE c.subject_id = ?1
           AND c.predicate = ?2
           AND (?3 IS NULL OR c.id <> ?3)
           AND c.status NOT IN ('rejected','archived')
         ORDER BY c.created_at DESC
         LIMIT ?4"
    );
    let limit = limit.clamp(1, 50) as i64;
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(
        params![
            subject_id.as_str(),
            predicate.as_str(),
            exclude.map(|id| id.as_str()),
            limit
        ],
        map_claim,
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 某文档贡献的全部 Claim（按写入顺序）。
pub fn list_by_document(conn: &Connection, document_id: &DocumentId) -> AppResult<Vec<ClaimRow>> {
    let sql = format!(
        "{CLAIM_SELECT}
         WHERE EXISTS (
             SELECT 1 FROM evidence e4
             WHERE e4.claim_id = c.id AND e4.document_id = ?1
         )
         ORDER BY c.created_at, c.id"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![document_id.as_str()], map_claim)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 按 id 集合批量取（Batch Tool，减少往返；见 PRD §35）。
pub fn get_many(conn: &Connection, ids: &[ClaimId]) -> AppResult<Vec<ClaimRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("{CLAIM_SELECT} WHERE c.id IN ({placeholders})");
    let mut statement = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::ToSql> = ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let rows = statement.query_map(params.as_slice(), map_claim)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Claim 总数。
pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM claims", [], |row| row.get(0))?)
}

/// 没有任何证据的 Claim 数（Knowledge Health 指标）。
///
/// 这是"知识不完整"的最直接信号：一条无法回答"凭什么"的断言。
pub fn count_without_evidence(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM claims c
         WHERE c.status NOT IN ('rejected','archived')
           AND NOT EXISTS (SELECT 1 FROM evidence e WHERE e.claim_id = c.id)",
        [],
        |row| row.get(0),
    )?)
}

/// 已被取代的 Claim 数。
pub fn count_superseded(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM claims WHERE status = 'superseded'",
        [],
        |row| row.get(0),
    )?)
}

/// 宾语未消解为实体的 Claim 数（Knowledge Health 指标）。
pub fn count_unresolved_objects(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM claims
         WHERE object_id IS NULL AND object_text IS NOT NULL AND trim(object_text) <> ''
           AND status NOT IN ('rejected','archived')",
        [],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::common::ids::EvidenceId;
    use crate::domain::evidence::evidence::EvidenceLevel;
    use crate::domain::knowledge::document::{Document, SourceType};
    use crate::infrastructure::db::tests::memory_db;
    use crate::infrastructure::{document_repository, entity_repository};

    fn seed(conn: &Connection) -> (EntityId, EntityId, DocumentId, Claim) {
        let subject = entity_repository::resolve_or_create(conn, "Rust").unwrap();
        let object = entity_repository::resolve_or_create(conn, "SQLite").unwrap();

        let content = "Rust uses SQLite in this project.";
        let document = Document {
            id: DocumentId::new(),
            title: "Design note".into(),
            content: content.into(),
            content_hash: Document::content_hash(content),
            source_type: SourceType::Note,
            source_uri: None,
            metadata: serde_json::json!({}),
            created_at: String::new(),
            updated_at: String::new(),
        };
        document_repository::insert(conn, &document).unwrap();
        document_repository::replace_chunks(
            conn,
            &document.id,
            &crate::domain::knowledge::chunk::chunk_document(content),
        )
        .unwrap();

        let claim = Claim {
            id: ClaimId::new(),
            subject_id: subject.id.clone(),
            predicate: ClaimPredicate::Uses,
            object: Some(ClaimObject::Entity(object.id.clone())),
            content: Some("Rust uses SQLite".into()),
            context: serde_json::json!({}),
            claim_type: ClaimType::Factual,
            polarity: Polarity::Positive,
            modality: Modality::Asserted,
            condition: None,
            confidence: Some(0.9),
            status: ClaimStatus::Candidate,
            valid_from: None,
            valid_until: None,
            recorded_at: String::new(),
            created_at: String::new(),
        };
        insert(conn, &claim).unwrap();

        (subject.id, object.id, document.id, claim)
    }

    fn add_evidence(conn: &Connection, claim_id: &ClaimId, document_id: &DocumentId, level: u8) {
        conn.execute(
            "INSERT INTO evidence(id,claim_id,document_id,quote,evidence_level)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                EvidenceId::new().as_str(),
                claim_id.as_str(),
                document_id.as_str(),
                "Rust uses SQLite",
                level as i64
            ],
        )
        .unwrap();
    }

    #[test]
    fn insert_and_read_round_trip_the_object() {
        let conn = memory_db();
        let (_, object_id, _, claim) = seed(&conn);

        let loaded = get(&conn, &claim.id).unwrap().unwrap();
        assert_eq!(loaded.claim.predicate, ClaimPredicate::Uses);
        assert_eq!(loaded.object_name.as_deref(), Some("SQLite"));
        assert_eq!(loaded.subject_name, "Rust");
        assert_eq!(loaded.claim.confidence, Some(0.9));
        assert_eq!(
            loaded.claim.object,
            Some(ClaimObject::Entity(object_id))
        );
        assert_eq!(loaded.evidence_count, 0);
    }

    #[test]
    fn display_text_prefers_extracted_content_then_falls_back_to_a_sentence() {
        let conn = memory_db();
        let (_, _, _, claim) = seed(&conn);
        let loaded = get(&conn, &claim.id).unwrap().unwrap();
        assert_eq!(loaded.display_text(), "Rust uses SQLite");

        conn.execute("UPDATE claims SET content = NULL WHERE id = ?1", params![claim.id.as_str()])
            .unwrap();
        let without_content = get(&conn, &claim.id).unwrap().unwrap();
        assert_eq!(without_content.display_text(), "Rust uses SQLite");
    }

    #[test]
    fn the_cheapest_evidence_becomes_the_primary_source() {
        let conn = memory_db();
        let (_, _, document_id, claim) = seed(&conn);
        add_evidence(&conn, &claim.id, &document_id, 3);
        add_evidence(&conn, &claim.id, &document_id, 1);

        let loaded = get(&conn, &claim.id).unwrap().unwrap();
        assert_eq!(loaded.evidence_count, 2);
        assert_eq!(loaded.source_document_title.as_deref(), Some("Design note"));
        assert_eq!(loaded.source_quote.as_deref(), Some("Rust uses SQLite"));
    }

    #[test]
    fn filters_narrow_the_result_set() {
        let conn = memory_db();
        let (subject_id, _, document_id, claim) = seed(&conn);

        let by_subject = list(
            &conn,
            &ClaimFilter {
                subject_id: Some(subject_id.clone()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_subject.len(), 1);

        let by_wrong_predicate = list(
            &conn,
            &ClaimFilter {
                predicate: Some(ClaimPredicate::Supports),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(by_wrong_predicate.is_empty());

        // 没有证据时按文档过滤查不到任何东西
        let by_document = list(
            &conn,
            &ClaimFilter {
                document_id: Some(document_id.clone()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(by_document.is_empty());

        add_evidence(&conn, &claim.id, &document_id, 1);
        let by_document = list(
            &conn,
            &ClaimFilter {
                document_id: Some(document_id),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(by_document.len(), 1);
    }

    #[test]
    fn candidates_exclude_self_and_rejected_claims() {
        let conn = memory_db();
        let (subject_id, object_id, _, claim) = seed(&conn);

        let same = Claim {
            id: ClaimId::new(),
            subject_id: subject_id.clone(),
            predicate: ClaimPredicate::Uses,
            object: Some(ClaimObject::Entity(object_id)),
            content: Some("Rust uses SQLite again".into()),
            context: serde_json::json!({}),
            claim_type: ClaimType::Factual,
            polarity: Polarity::Positive,
            modality: Modality::Asserted,
            condition: None,
            confidence: None,
            status: ClaimStatus::Candidate,
            valid_from: None,
            valid_until: None,
            recorded_at: String::new(),
            created_at: String::new(),
        };
        insert(&conn, &same).unwrap();

        let candidates =
            find_candidates(&conn, &subject_id, ClaimPredicate::Uses, Some(&claim.id), 10).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].claim.id, same.id);

        conn.execute(
            "UPDATE claims SET status = 'rejected' WHERE id = ?1",
            params![same.id.as_str()],
        )
        .unwrap();
        assert!(
            find_candidates(&conn, &subject_id, ClaimPredicate::Uses, Some(&claim.id), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn batch_lookup_returns_claims_in_the_requested_set() {
        let conn = memory_db();
        let (_, _, _, claim) = seed(&conn);
        let found = get_many(&conn, &[claim.id.clone(), ClaimId::from_raw("missing")]).unwrap();
        assert_eq!(found.len(), 1);
        assert!(get_many(&conn, &[]).unwrap().is_empty());
    }

    #[test]
    fn health_counts_track_the_real_tables() {
        let conn = memory_db();
        let (_, _, document_id, claim) = seed(&conn);

        assert_eq!(count(&conn).unwrap(), 1);
        assert_eq!(count_without_evidence(&conn).unwrap(), 1);
        assert_eq!(count_unresolved_objects(&conn).unwrap(), 0);
        assert_eq!(count_superseded(&conn).unwrap(), 0);

        add_evidence(&conn, &claim.id, &document_id, 1);
        assert_eq!(count_without_evidence(&conn).unwrap(), 0);

        conn.execute(
            "UPDATE claims SET status = 'superseded' WHERE id = ?1",
            params![claim.id.as_str()],
        )
        .unwrap();
        assert_eq!(count_superseded(&conn).unwrap(), 1);
    }

    #[test]
    fn unresolved_objects_are_counted_separately() {
        let conn = memory_db();
        let subject = entity_repository::resolve_or_create(&conn, "Rust").unwrap();
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
            recorded_at: String::new(),
            created_at: String::new(),
        };
        insert(&conn, &claim).unwrap();
        assert_eq!(count_unresolved_objects(&conn).unwrap(), 1);
        let _ = EvidenceLevel::Quote;
    }
}
