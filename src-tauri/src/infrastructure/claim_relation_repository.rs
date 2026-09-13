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
use crate::domain::evolution::decision::EvolutionTransition;
use crate::domain::knowledge::claim::ClaimStatus;
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

// 审核动作（Accept / Reject / Reset）与其状态迁移规则已上移到领域层：
// 见 `domain::evolution::decision::{ReviewAction, EvolutionTransition}`（K-002）。

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

/// 读取某条 Claim 的当前状态（轻量查询，供决策编排使用）。
pub fn claim_status(conn: &Connection, claim_id: &ClaimId) -> AppResult<Option<ClaimStatus>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT status FROM claims WHERE id = ?1",
            params![claim_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|value| value.parse::<ClaimStatus>()).transpose()
}

/// 应用一次演化决策（K-002）：**纯持久化**。
///
/// 本函数不含任何「什么关系该把 Claim 改成什么状态」的业务判断——那由
/// [`EvolutionTransition::plan`] 在领域层决定。Repository 只负责把结果写下去。
/// 它仍是整个系统里唯一会写 `claims.status` 的地方（INV-08）。
pub fn apply_decision(
    conn: &Connection,
    relation_id: &ClaimRelationId,
    relationship: ClaimRelationType,
    transition: &EvolutionTransition,
) -> AppResult<ClaimRelationRow> {
    let current = get(conn, relation_id)?
        .ok_or_else(|| AppError::NotFound(format!("演化关系 {relation_id} 不存在")))?;

    // 受影响方在本次决策前的状态：既供事件审计，也在下面写回锚点。
    let target_status_before = claim_status(conn, &current.target_claim_id)?;

    conn.execute(
        "UPDATE claim_relations SET relationship = ?1 WHERE id = ?2",
        params![relationship.as_str(), relation_id.as_str()],
    )?;

    // 记录回滚锚点：是否记录、记录什么，完全由领域层决定（避免重复确认覆盖锚点）。
    if let Some(anchor) = transition.record_previous_status {
        conn.execute(
            "UPDATE claim_relations SET target_previous_status = ?1 WHERE id = ?2",
            params![anchor.as_str(), relation_id.as_str()],
        )?;
    }

    // 目标 Claim 的状态迁移。
    // - 置为 superseded：无守卫（幂等）。
    // - 其余（精确恢复）：带 `status = 'superseded'` 守卫，只作用于确实被取代过的行（INV-09）。
    if let Some(new_status) = transition.target_status {
        let affected = if new_status == ClaimStatus::Superseded {
            conn.execute(
                "UPDATE claims SET status = 'superseded' WHERE id = ?1",
                params![current.target_claim_id.as_str()],
            )?
        } else {
            conn.execute(
                "UPDATE claims SET status = ?1 WHERE id = ?2 AND status = 'superseded'",
                params![new_status.as_str(), current.target_claim_id.as_str()],
            )?
        };

        // 契约 §52：不变量失败必须整体回滚，而不是把 relation 先保存下来。
        // 取代是唯一"必须真正改到行"的迁移；一行都没改到说明数据不一致。
        if new_status == ClaimStatus::Superseded && affected == 0 {
            return Err(AppError::Domain(format!(
                "违反知识契约：目标 Claim {} 不存在，无法执行取代（本次决策将整体回滚）",
                current.target_claim_id
            )));
        }
    }

    conn.execute(
        "UPDATE claim_relations SET status = ?1, reviewed_at = datetime('now') WHERE id = ?2",
        params![transition.relation_status.as_str(), relation_id.as_str()],
    )?;

    // CORE-005：把本次决策追加为**不可变**事件。Revert（reset）在这里同样产生
    // 新事件，而不是抹掉历史，因此 Timeline 能完整回放"知识为什么变化"。
    let target_status_after = claim_status(conn, &current.target_claim_id)?;
    conn.execute(
        "INSERT INTO claim_relation_events(
            id, relation_id, relationship, status,
            target_claim_id, target_status_before, target_status_after, note
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            uuid::Uuid::new_v4().to_string(),
            relation_id.as_str(),
            relationship.as_str(),
            transition.relation_status.as_str(),
            current.target_claim_id.as_str(),
            target_status_before.map(|status| status.as_str().to_string()),
            target_status_after.map(|status| status.as_str().to_string()),
            current.reason,
        ],
    )?;

    get(conn, relation_id)?
        .ok_or_else(|| AppError::Internal("关系刚更新却读不到".into()))
}

/// 一条演化事件（CORE-005）。只追加、不修改，用于审计与 Timeline。
#[derive(Debug, Clone)]
pub struct ClaimRelationEvent {
    pub id: String,
    pub relation_id: ClaimRelationId,
    pub relationship: ClaimRelationType,
    pub status: ClaimRelationStatus,
    pub target_claim_id: Option<ClaimId>,
    pub target_status_before: Option<String>,
    pub target_status_after: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
}

/// 某条关系的全部演化事件（时间正序，含 Revert）。
pub fn list_events(
    conn: &Connection,
    relation_id: &ClaimRelationId,
) -> AppResult<Vec<ClaimRelationEvent>> {
    let mut statement = conn.prepare(
        "SELECT id, relation_id, relationship, status, target_claim_id,
                target_status_before, target_status_after, note, created_at
         FROM claim_relation_events
         WHERE relation_id = ?1
         ORDER BY created_at ASC, rowid ASC",
    )?;
    let rows = statement.query_map(params![relation_id.as_str()], |row| {
        Ok(ClaimRelationEvent {
            id: row.get(0)?,
            relation_id: parse_col::<ClaimRelationId>(row, 1)?,
            relationship: parse_col::<ClaimRelationType>(row, 2)?,
            status: parse_col::<ClaimRelationStatus>(row, 3)?,
            target_claim_id: row.get::<_, Option<String>>(4)?.map(ClaimId::from_raw),
            target_status_before: row.get(5)?,
            target_status_after: row.get(6)?,
            note: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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
    use crate::domain::evolution::decision::{EvolutionTransition, ReviewAction};
    use crate::domain::ontology::predicate::ClaimPredicate;
    use crate::infrastructure::db::tests::memory_db;
    use crate::infrastructure::{claim_repository, entity_repository};

    /// 测试辅助：与应用层同构地执行一次决策（领域规划 → 持久化）。
    ///
    /// 生产路径由 `application::evolution_service::decide_relation` 编排；
    /// Repository 本身只暴露纯持久化的 `apply_decision`。
    fn decide(
        conn: &Connection,
        relation_id: &ClaimRelationId,
        action: ReviewAction,
        relationship_override: Option<ClaimRelationType>,
    ) -> AppResult<ClaimRelationRow> {
        let current = get(conn, relation_id)?
            .ok_or_else(|| AppError::NotFound(format!("演化关系 {relation_id} 不存在")))?;
        let relationship = relationship_override.unwrap_or(current.relationship);
        // 注意：测试模块内也有同名函数 `claim_status`（返回 String），这里显式用仓储版。
        let target_status = super::claim_status(conn, &current.target_claim_id)?;
        let anchor = current
            .target_previous_status
            .as_deref()
            .map(str::parse::<ClaimStatus>)
            .transpose()?;
        let transition = EvolutionTransition::plan(relationship, action, target_status, anchor);
        apply_decision(conn, relation_id, relationship, &transition)
    }

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
                observed_at: None,
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
            ReviewAction::Accept,
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

        decide(&conn, &id, ReviewAction::Accept, Some(ClaimRelationType::Supersedes))
            .unwrap();
        assert_eq!(claim_status(&conn, &target), "superseded");

        decide(&conn, &id, ReviewAction::Reset, None).unwrap();
        assert_eq!(claim_status(&conn, &target), "verified", "必须精确恢复（INV-09）");
    }

    #[test]
    fn reclassifying_an_accepted_supersedes_restores_the_target() {
        let conn = memory_db();
        let (_, target, id) = seed_relation(&conn);

        decide(&conn, &id, ReviewAction::Accept, Some(ClaimRelationType::Supersedes)).unwrap();
        assert_eq!(claim_status(&conn, &target), "superseded");

        // 改判为 supplements 且仍 accepted：必须回滚被取代的 Claim（INV-09）。
        let updated = decide(
            &conn,
            &id,
            ReviewAction::Accept,
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
        decide(&conn, &id, ReviewAction::Accept, Some(ClaimRelationType::Supersedes))
            .unwrap();
        // 再次确认：不能把 'superseded' 记成 previous_status
        decide(&conn, &id, ReviewAction::Accept, Some(ClaimRelationType::Supersedes))
            .unwrap();
        let row = get(&conn, &id).unwrap().unwrap();
        assert_eq!(row.target_previous_status.as_deref(), Some("verified"));

        decide(&conn, &id, ReviewAction::Reset, None).unwrap();
        assert_eq!(claim_status(&conn, &target), "verified");
    }

    #[test]
    fn rejecting_changes_no_claim_at_all() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);

        let rejected = decide(&conn, &id, ReviewAction::Reject, None).unwrap();
        assert_eq!(rejected.status, ClaimRelationStatus::Rejected);
        assert_eq!(claim_status(&conn, &source), "verified");
        assert_eq!(claim_status(&conn, &target), "verified");
    }

    #[test]
    fn a_contradiction_never_touches_claim_status() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);
        decide(&conn, &id, ReviewAction::Accept, None).unwrap();
        assert_eq!(claim_status(&conn, &source), "verified");
        assert_eq!(claim_status(&conn, &target), "verified");
    }

    #[test]
    fn unknown_relations_are_not_found() {
        let conn = memory_db();
        let err = decide(
            &conn,
            &ClaimRelationId::from_raw("missing"),
            ReviewAction::Accept,
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
            observed_at: None,
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

        decide(&conn, &id, ReviewAction::Accept, None).unwrap();
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
        decide(&conn, &id, ReviewAction::Accept, None).unwrap();

        let rows = list_for_claim(&conn, &source).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, ClaimRelationStatus::Accepted);

        let rows = list_for_claim(&conn, &target).unwrap();
        assert_eq!(rows.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Phase 2：演化不变量 / 事件日志 / 事务原子性
    // -----------------------------------------------------------------------

    /// TEST-002：全库演化不变量自检。
    fn assert_evolution_invariants(conn: &Connection) {
        // INV-08：任何 superseded 的 claim 必须有一条 accepted 的 supersedes 指向它。
        let orphan: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM claims c
                 WHERE c.status = 'superseded'
                   AND NOT EXISTS (
                     SELECT 1 FROM claim_relations r
                     WHERE r.target_claim_id = c.id
                       AND r.relationship = 'supersedes'
                       AND r.status = 'accepted'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphan, 0, "存在没有 accepted supersedes 支撑的 superseded claim");

        // 反向：accepted 的 supersedes 的 target 必须是 superseded。
        let dangling: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM claim_relations r
                 JOIN claims c ON c.id = r.target_claim_id
                 WHERE r.relationship = 'supersedes' AND r.status = 'accepted'
                   AND c.status <> 'superseded'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0, "accepted supersedes 的 target 却不是 superseded");
    }

    #[test]
    fn every_decision_appends_an_immutable_evolution_event() {
        let conn = memory_db();
        let (_, target, id) = seed_relation(&conn);

        decide(&conn, &id, ReviewAction::Accept, Some(ClaimRelationType::Supersedes)).unwrap();
        decide(&conn, &id, ReviewAction::Reset, None).unwrap();

        let events = list_events(&conn, &id).unwrap();
        assert_eq!(events.len(), 2, "Revert 必须新增事件，而不是覆盖历史");
        assert_eq!(events[0].status, ClaimRelationStatus::Accepted);
        assert_eq!(events[0].relationship, ClaimRelationType::Supersedes);
        assert_eq!(events[0].target_status_before.as_deref(), Some("verified"));
        assert_eq!(events[0].target_status_after.as_deref(), Some("superseded"));

        assert_eq!(events[1].status, ClaimRelationStatus::Candidate);
        assert_eq!(events[1].target_status_before.as_deref(), Some("superseded"));
        assert_eq!(events[1].target_status_after.as_deref(), Some("verified"));

        assert_eq!(claim_status(&conn, &target), "verified");
        assert_evolution_invariants(&conn);
    }

    #[test]
    fn rejects_and_non_supersede_accepts_keep_invariants_and_touch_no_status() {
        let conn = memory_db();
        let (source, target, id) = seed_relation(&conn);

        // 拒绝：不改任何 claim 状态。
        decide(&conn, &id, ReviewAction::Reject, None).unwrap();
        assert_eq!(claim_status(&conn, &source), "verified");
        assert_eq!(claim_status(&conn, &target), "verified");
        // contradicts + accept：同样不产生 superseded。
        decide(&conn, &id, ReviewAction::Accept, None).unwrap();
        assert_eq!(claim_status(&conn, &target), "verified");
        assert_evolution_invariants(&conn);
    }

    /// CORE-004：`decide` 可组合进调用方事务，且整体回滚不留部分状态。
    #[test]
    fn decide_is_atomic_when_the_caller_rolls_back() {
        let mut conn = memory_db();
        let (_, target, id) = seed_relation(&conn);

        {
            let tx = conn.transaction().unwrap();
            decide(&tx, &id, ReviewAction::Accept, Some(ClaimRelationType::Supersedes)).unwrap();
            assert_eq!(claim_status(&tx, &target), "superseded");
            tx.rollback().unwrap();
        }

        assert_eq!(claim_status(&conn, &target), "verified", "回滚后不得残留状态变更");
        assert!(
            list_events(&conn, &id).unwrap().is_empty(),
            "回滚后不得残留事件"
        );
    }
}
