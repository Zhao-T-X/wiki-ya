//! Claim 演化关系的持久化，以及 `superseded` 的**唯一**状态迁移入口。
//!
//! INV-08 与 INV-09 都落在这个文件里：
//!
//! - 只有 `supersedes` + `accepted` 能让一条 Claim 变成 `superseded`
//! - 取消这个确认时，必须按 `target_previous_status` **精确**恢复原状态
//!
//! 因此任何"直接 `UPDATE claims SET status`"的代码都应当被视为 bug。

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::common::ids::{ClaimId, ClaimRelationId};
use crate::domain::evolution::classifier::{ClaimRelationStatus, ClaimRelationType};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::{opt_f32_col, parse_col};

/// 关系 + 两端 Claim 的可读陈述。
#[derive(Debug, Clone)]
pub struct ClaimRelationRow {
    pub id: ClaimRelationId,
    pub source_claim_id: ClaimId,
    pub source_text: String,
    pub target_claim_id: ClaimId,
    pub target_text: String,
    pub relationship: ClaimRelationType,
    pub status: ClaimRelationStatus,
    pub confidence: Option<f32>,
    pub reason: Option<String>,
    pub suggested_action: Option<String>,
    pub target_previous_status: Option<String>,
    pub created_at: String,
}

/// 审核决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationDecision {
    /// 接受：产生事实效果（仅 `supersedes` 会改变当前知识）。
    Accept,
    /// 拒绝：关系作废，**不改动任何 Claim**。
    Reject,
    /// 撤销：回到待审状态；若此前确认过取代，则恢复旧 Claim 的状态。
    Reset,
}

impl RelationDecision {
    pub fn status(&self) -> ClaimRelationStatus {
        match self {
            RelationDecision::Accept => ClaimRelationStatus::Accepted,
            RelationDecision::Reject => ClaimRelationStatus::Rejected,
            RelationDecision::Reset => ClaimRelationStatus::Candidate,
        }
    }
}

/// 两端 Claim 的可读陈述由 SQL 直接拼出（与 `claim_repository::display_text` 同口径）。
const RELATION_SELECT: &str = "
SELECT
  cr.id, cr.source_claim_id, cr.target_claim_id, cr.relationship, cr.status,
  cr.confidence, cr.reason, cr.suggested_action, cr.target_previous_status, cr.created_at,
  COALESCE(NULLIF(trim(cs.content), ''),
           trim(s.name || ' ' || cs.predicate || ' ' || COALESCE(os.name, cs.object_text, ''))) AS source_text,
  COALESCE(NULLIF(trim(ct.content), ''),
           trim(t.name || ' ' || ct.predicate || ' ' || COALESCE(ot.name, ct.object_text, ''))) AS target_text
FROM claim_relations cr
JOIN claims cs ON cs.id = cr.source_claim_id
JOIN entities s ON s.id = cs.subject_id
LEFT JOIN entities os ON os.id = cs.object_id
JOIN claims ct ON ct.id = cr.target_claim_id
JOIN entities t ON t.id = ct.subject_id
LEFT JOIN entities ot ON ot.id = ct.object_id
";

/// 审核队列排序：先按关系严重程度，再按时间倒序。
const REVIEW_ORDER: &str = "
ORDER BY CASE cr.relationship
    WHEN 'duplicate' THEN 0
    WHEN 'supersedes' THEN 1
    WHEN 'contradicts' THEN 2
    WHEN 'coexists' THEN 3
    WHEN 'supplements' THEN 4
    ELSE 5
  END, cr.created_at DESC
";

fn map_relation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaimRelationRow> {
    Ok(ClaimRelationRow {
        id: parse_col::<ClaimRelationId>(row, 0)?,
        source_claim_id: parse_col::<ClaimId>(row, 1)?,
        target_claim_id: parse_col::<ClaimId>(row, 2)?,
        relationship: parse_col::<ClaimRelationType>(row, 3)?,
        status: parse_col::<ClaimRelationStatus>(row, 4)?,
        confidence: opt_f32_col(row, 5)?,
        reason: row.get(6)?,
        suggested_action: row.get(7)?,
        target_previous_status: row.get(8)?,
        created_at: row.get(9)?,
        source_text: row.get(10)?,
        target_text: row.get(11)?,
    })
}

/// 写入一条关系；已存在同三元组时返回 `None`（不重复上报）。
pub fn insert(
    conn: &Connection,
    source_claim_id: &ClaimId,
    target_claim_id: &ClaimId,
    relationship: ClaimRelationType,
    status: ClaimRelationStatus,
    confidence: Option<f32>,
    reason: Option<&str>,
    suggested_action: Option<&str>,
) -> AppResult<Option<ClaimRelationId>> {
    if source_claim_id == target_claim_id {
        return Err(AppError::Domain("Claim 不能与自己建立演化关系".into()));
    }

    let id = ClaimRelationId::new();
    let affected = conn.execute(
        "INSERT OR IGNORE INTO claim_relations(
            id, source_claim_id, target_claim_id, relationship, status,
            confidence, reason, suggested_action, created_by
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'system')",
        params![
            id.as_str(),
            source_claim_id.as_str(),
            target_claim_id.as_str(),
            relationship.as_str(),
            status.as_str(),
            confidence.map(f64::from),
            reason,
            suggested_action,
        ],
    )?;

    Ok(if affected == 0 { None } else { Some(id) })
}

/// 这一对 Claim 之间是否已经有关系行（任一方向）。
///
/// 同一个事实不该被两个方向各上报一次——那会让审核队列凭空翻倍。
pub fn pair_exists(conn: &Connection, left: &ClaimId, right: &ClaimId) -> AppResult<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM claim_relations
             WHERE (source_claim_id = ?1 AND target_claim_id = ?2)
                OR (source_claim_id = ?2 AND target_claim_id = ?1)
             LIMIT 1",
            params![left.as_str(), right.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// 按 id 取关系。
pub fn get(conn: &Connection, id: &ClaimRelationId) -> AppResult<Option<ClaimRelationRow>> {
    let sql = format!("{RELATION_SELECT} WHERE cr.id = ?1");
    Ok(conn
        .query_row(&sql, params![id.as_str()], map_relation)
        .optional()?)
}

/// 待审核队列。
pub fn list_pending(conn: &Connection, limit: usize) -> AppResult<Vec<ClaimRelationRow>> {
    let sql = format!(
        "{RELATION_SELECT} WHERE cr.status = 'candidate' {REVIEW_ORDER} LIMIT ?1"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![limit.clamp(1, 200) as i64], map_relation)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 某条 Claim 相关的全部关系（accepted 的排前面，其余按时间倒序）。
pub fn list_for_claim(conn: &Connection, claim_id: &ClaimId) -> AppResult<Vec<ClaimRelationRow>> {
    let sql = format!(
        "{RELATION_SELECT}
         WHERE cr.source_claim_id = ?1 OR cr.target_claim_id = ?1
         ORDER BY CASE cr.status WHEN 'accepted' THEN 0 WHEN 'candidate' THEN 1 ELSE 2 END,
                  cr.created_at DESC"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![claim_id.as_str()], map_relation)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 提交一次审核决策。
///
/// 这是整个系统里唯一会写 `claims.status` 的地方（INV-08）。
pub fn decide(
    conn: &Connection,
    relation_id: &ClaimRelationId,
    decision: RelationDecision,
    relationship_override: Option<ClaimRelationType>,
) -> AppResult<ClaimRelationRow> {
    let current = get(conn, relation_id)?
        .ok_or_else(|| AppError::NotFound(format!("演化关系 {relation_id} 不存在")))?;

    // 允许审核时修正关系类型（用户可能判断"这不是取代而是补充"）。
    let relationship = relationship_override.unwrap_or(current.relationship);
    let new_status = decision.status();

    conn.execute(
        "UPDATE claim_relations SET relationship = ?1 WHERE id = ?2",
        params![relationship.as_str(), relation_id.as_str()],
    )?;

    // 唯一会产生"取代"副作用的条件：最终关系是 supersedes 且被接受。
    // 用单一真值驱动，避免"改判成其它关系 + accepted"这种组合既不置位也不回滚（INV-08/09）。
    let is_accepted_supersedes = relationship == ClaimRelationType::Supersedes
        && new_status == ClaimRelationStatus::Accepted;

    if is_accepted_supersedes {
        // 记录"被取代前的状态"，只记录一次：
        // 如果目标已经是 superseded（重复确认），再记录会把 superseded 存成
        // previous_status，导致回滚时恢复成一个错误的状态。
        let target_status: Option<String> = conn
            .query_row(
                "SELECT status FROM claims WHERE id = ?1",
                params![current.target_claim_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(status) = target_status {
            if status != "superseded" {
                conn.execute(
                    "UPDATE claim_relations SET target_previous_status = ?1 WHERE id = ?2",
                    params![status, relation_id.as_str()],
                )?;
            }
            conn.execute(
                "UPDATE claims SET status = 'superseded' WHERE id = ?1",
                params![current.target_claim_id.as_str()],
            )?;
        }
    } else {
        // 不再是"已确认的取代"：无论是被拒绝 / 撤销，还是**被改判成其它关系**，
        // 都必须按记录的原状态精确恢复（INV-09），否则 Claim 会永久卡在 superseded。
        if let Some(previous) = current.target_previous_status.as_deref() {
            conn.execute(
                "UPDATE claims SET status = ?1
                 WHERE id = ?2 AND status = 'superseded'",
                params![previous, current.target_claim_id.as_str()],
            )?;
        }
    }

    conn.execute(
        "UPDATE claim_relations SET status = ?1, reviewed_at = datetime('now') WHERE id = ?2",
        params![new_status.as_str(), relation_id.as_str()],
    )?;

    get(conn, relation_id)?
        .ok_or_else(|| AppError::Internal("关系刚更新却读不到".into()))
}

/// 潜在重复数（Knowledge Health）。
pub fn count_potential_duplicates(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM claim_relations
         WHERE relationship IN ('duplicate','coexists') AND status <> 'rejected'",
        [],
        |row| row.get(0),
    )?)
}

/// 未解决的冲突数（Knowledge Health）。
pub fn count_unresolved_conflicts(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM claim_relations
         WHERE relationship IN ('contradicts','supersedes') AND status = 'candidate'",
        [],
        |row| row.get(0),
    )?)
}

/// 待审核总数。
pub fn count_pending(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM claim_relations WHERE status = 'candidate'",
        [],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::claim::{
        Claim, ClaimObject, ClaimStatus, ClaimType, Modality, Polarity,
    };
    use crate::domain::ontology::predicate::ClaimPredicate;
    use crate::infrastructure::db::tests::memory_db;
    use crate::infrastructure::{claim_repository, entity_repository};

    /// 造两条"同一主语同一谓语、宾语不同"的 Claim。
    fn two_claims(conn: &Connection) -> (ClaimId, ClaimId) {
        let subject = entity_repository::resolve_or_create(conn, "OpenAI").unwrap();
        let first = entity_repository::resolve_or_create(conn, "Sam").unwrap();
        let second = entity_repository::resolve_or_create(conn, "Alice").unwrap();

        let mut ids = Vec::new();
        for object in [first.id, second.id] {
            let claim = Claim {
                id: ClaimId::new(),
                subject_id: subject.id.clone(),
                predicate: ClaimPredicate::Is,
                object: Some(ClaimObject::Entity(object)),
                content: None,
                context: serde_json::json!({}),
                claim_type: ClaimType::Factual,
                polarity: Polarity::Positive,
                modality: Modality::Asserted,
                condition: None,
                confidence: None,
                status: ClaimStatus::Verified,
                valid_from: None,
                valid_until: None,
                recorded_at: String::new(),
                created_at: String::new(),
            };
            claim_repository::insert(conn, &claim).unwrap();
            ids.push(claim.id);
        }
        (ids[0].clone(), ids[1].clone())
    }

    fn claim_status(conn: &Connection, id: &ClaimId) -> String {
        conn.query_row(
            "SELECT status FROM claims WHERE id = ?1",
            params![id.as_str()],
            |row| {
                row.get::<_, String>(0)
            },
        )
        .unwrap()
    }

    fn seed_relation(conn: &Connection) -> (ClaimId, ClaimId, ClaimRelationId) {
        let (source, target) = two_claims(conn);
        let id = insert(
            conn,
            &source,
            &target,
            ClaimRelationType::Contradicts,
            ClaimRelationStatus::Candidate,
            Some(0.55),
            Some("取值发生变化"),
            Some("review"),
        )
        .unwrap()
        .unwrap();
        (source, target, id)
    }

    #[test]
    fn insert_is_idempotent_on_the_same_triple() {
        let conn = memory_db();
        let (source, target, _) = seed_relation(&conn);
        let again = insert(
            &conn,
            &source,
            &target,
            ClaimRelationType::Contradicts,
            ClaimRelationStatus::Candidate,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(again.is_none(), "同一个三元组不应重复上报（INV-07）");
    }

    #[test]
    fn self_relations_are_rejected() {
        let conn = memory_db();
        let (source, _) = two_claims(&conn);
        let err = insert(
            &conn,
            &source,
            &source,
            ClaimRelationType::Duplicate,
            ClaimRelationStatus::Accepted,
            None,
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(err.code(), "DOMAIN_RULE_VIOLATION");
    }

    #[test]
    fn pair_exists_looks_in_both_directions() {
        let conn = memory_db();
        let (source, target, _) = seed_relation(&conn);
        assert!(pair_exists(&conn, &source, &target).unwrap());
        assert!(pair_exists(&conn, &target, &source).unwrap());
    }

    #[test]
    fn accepting_supersedes_moves_the_older_claim_to_superseded() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);

        let updated = decide(
            &conn,
            &id,
            RelationDecision::Accept,
            Some(ClaimRelationType::Supersedes),
        )
        .unwrap();

        assert_eq!(updated.status, ClaimRelationStatus::Accepted);
        assert_eq!(updated.relationship, ClaimRelationType::Supersedes);
        assert_eq!(updated.target_previous_status.as_deref(), Some("verified"));
        assert_eq!(claim_status(&conn, &target), "superseded");
        // 发起方（新知识）不受影响
        assert_eq!(claim_status(&conn, &source), "verified");
    }

    #[test]
    fn resetting_restores_the_exact_previous_status() {
        let conn = memory_db();
        let (_, target, id) = seed_relation(&conn);

        decide(&conn, &id, RelationDecision::Accept, Some(ClaimRelationType::Supersedes))
            .unwrap();
        assert_eq!(claim_status(&conn, &target), "superseded");

        decide(&conn, &id, RelationDecision::Reset, None).unwrap();
        assert_eq!(claim_status(&conn, &target), "verified", "必须精确恢复（INV-09）");
    }

    #[test]
    fn reclassifying_an_accepted_supersedes_restores_the_target() {
        let conn = memory_db();
        let (_, target, id) = seed_relation(&conn);

        decide(&conn, &id, RelationDecision::Accept, Some(ClaimRelationType::Supersedes)).unwrap();
        assert_eq!(claim_status(&conn, &target), "superseded");

        // 改判为 supplements 且仍 accepted：必须回滚被取代的 Claim（INV-09）。
        let updated = decide(
            &conn,
            &id,
            RelationDecision::Accept,
            Some(ClaimRelationType::Supplements),
        )
        .unwrap();
        assert_eq!(updated.relationship, ClaimRelationType::Supplements);
        assert_eq!(updated.status, ClaimRelationStatus::Accepted);
        assert_eq!(
            claim_status(&conn, &target),
            "verified",
            "改判成非取代关系后，target 不能继续卡在 superseded"
        );
    }

    #[test]
    fn repeating_accept_does_not_corrupt_the_rollback_anchor() {
        let conn = memory_db();
        let (_, target, id) = seed_relation(&conn);
        decide(&conn, &id, RelationDecision::Accept, Some(ClaimRelationType::Supersedes))
            .unwrap();
        // 再次确认：不能把 'superseded' 记成 previous_status
        decide(&conn, &id, RelationDecision::Accept, Some(ClaimRelationType::Supersedes))
            .unwrap();
        let row = get(&conn, &id).unwrap().unwrap();
        assert_eq!(row.target_previous_status.as_deref(), Some("verified"));

        decide(&conn, &id, RelationDecision::Reset, None).unwrap();
        assert_eq!(claim_status(&conn, &target), "verified");
    }

    #[test]
    fn rejecting_changes_no_claim_at_all() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);

        let rejected = decide(&conn, &id, RelationDecision::Reject, None).unwrap();
        assert_eq!(rejected.status, ClaimRelationStatus::Rejected);
        assert_eq!(claim_status(&conn, &source), "verified");
        assert_eq!(claim_status(&conn, &target), "verified");
    }

    #[test]
    fn a_contradiction_never_touches_claim_status() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);
        decide(&conn, &id, RelationDecision::Accept, None).unwrap();
        assert_eq!(claim_status(&conn, &source), "verified");
        assert_eq!(claim_status(&conn, &target), "verified");
    }

    #[test]
    fn unknown_relations_are_not_found() {
        let conn = memory_db();
        let err = decide(
            &conn,
            &ClaimRelationId::from_raw("missing"),
            RelationDecision::Accept,
            None,
        )
        .unwrap_err();
        assert_eq!(err.code(), "NOT_FOUND");
    }

    #[test]
    fn pending_queue_is_ordered_by_severity_then_recency() {
        let conn = memory_db();
        let (source, target) = two_claims(&conn);
        let third = entity_repository::resolve_or_create(&conn, "Bob").unwrap();
        let subject = entity_repository::resolve_or_create(&conn, "OpenAI").unwrap();
        let claim3 = Claim {
            id: ClaimId::new(),
            subject_id: subject.id,
            predicate: ClaimPredicate::Is,
            object: Some(ClaimObject::Entity(third.id)),
            content: None,
            context: serde_json::json!({}),
            claim_type: ClaimType::Factual,
            polarity: Polarity::Positive,
            modality: Modality::Asserted,
            condition: None,
            confidence: None,
            status: ClaimStatus::Verified,
            valid_from: None,
            valid_until: None,
            recorded_at: String::new(),
            created_at: String::new(),
        };
        claim_repository::insert(&conn, &claim3).unwrap();

        insert(
            &conn,
            &source,
            &target,
            ClaimRelationType::Coexists,
            ClaimRelationStatus::Accepted,
            None,
            None,
            None,
        )
        .unwrap();
        insert(
            &conn,
            &claim3.id,
            &target,
            ClaimRelationType::Contradicts,
            ClaimRelationStatus::Candidate,
            None,
            None,
            None,
        )
        .unwrap();

        let pending = list_pending(&conn, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].relationship, ClaimRelationType::Contradicts);
        assert!(pending[0].source_text.contains("OpenAI"));
        assert_eq!(count_pending(&conn).unwrap(), 1);
    }

    #[test]
    fn health_counters_follow_the_relations_table() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);
        assert_eq!(count_potential_duplicates(&conn).unwrap(), 0);
        assert_eq!(count_unresolved_conflicts(&conn).unwrap(), 1);

        decide(&conn, &id, RelationDecision::Accept, None).unwrap();
        assert_eq!(count_unresolved_conflicts(&conn).unwrap(), 0);

        insert(
            &conn,
            &target,
            &source,
            ClaimRelationType::Duplicate,
            ClaimRelationStatus::Candidate,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(count_potential_duplicates(&conn).unwrap(), 1);
    }

    #[test]
    fn relations_for_a_claim_include_both_directions_with_accepted_first() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);
        decide(&conn, &id, RelationDecision::Accept, None).unwrap();

        let rows = list_for_claim(&conn, &source).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, ClaimRelationStatus::Accepted);

        let rows = list_for_claim(&conn, &target).unwrap();
        assert_eq!(rows.len(), 1);
    }
}
