//! `reviews` 表的持久化。
//!
//! ## 与 Claim 演化审核的关系
//!
//! Phase 1 的 Review 队列**直接来自 `claim_relations`**（那才是需要用户
//! 决策的东西），本表用于承载尚未落成具体知识对象的提案
//! （研究结论、Agent 提案、实体合并建议等，见 [`ReviewTarget`]）。
//!
//! 两者共用同一个 UI 队列，但存储分开：一条"关系待确认"已经有了
//! 自己的表，再往里塞一份副本就会产生两个真相来源。

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::common::ids::ReviewId;
use crate::domain::review::review::{ReviewStatus, ReviewTarget};
use crate::error::AppResult;
use crate::infrastructure::db::{json_col, parse_col};

/// 一条待审记录。
#[derive(Debug, Clone)]
pub struct ReviewRecord {
    pub id: ReviewId,
    pub target_type: ReviewTarget,
    pub target_id: String,
    pub proposal: serde_json::Value,
    pub status: ReviewStatus,
    pub created_at: String,
    pub reviewed_at: Option<String>,
}

/// 登记一条提案。
pub fn insert_pending(
    conn: &Connection,
    target_type: ReviewTarget,
    target_id: &str,
    proposal: serde_json::Value,
) -> AppResult<ReviewId> {
    let id = ReviewId::new();
    conn.execute(
        "INSERT INTO reviews(id, target_type, target_id, proposal_json, status)
         VALUES(?1,?2,?3,?4,'pending')",
        params![
            id.as_str(),
            target_type.as_str(),
            target_id,
            serde_json::to_string(&proposal)?,
        ],
    )?;
    Ok(id)
}

const REVIEW_COLUMNS: &str =
    "id, target_type, target_id, proposal_json, status, created_at, reviewed_at";

fn map_review(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewRecord> {
    Ok(ReviewRecord {
        id: parse_col::<ReviewId>(row, 0)?,
        target_type: parse_col::<ReviewTarget>(row, 1)?,
        target_id: row.get(2)?,
        proposal: json_col(row, 3)?,
        status: parse_col::<ReviewStatus>(row, 4)?,
        created_at: row.get(5)?,
        reviewed_at: row.get(6)?,
    })
}

/// 待审记录（最新优先）。
pub fn list_pending(conn: &Connection, limit: usize) -> AppResult<Vec<ReviewRecord>> {
    let sql = format!(
        "SELECT {REVIEW_COLUMNS} FROM reviews
         WHERE status = 'pending'
         ORDER BY created_at DESC
         LIMIT ?1"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![limit.clamp(1, 200) as i64], map_review)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 按 id 取记录。
pub fn get(conn: &Connection, id: &ReviewId) -> AppResult<Option<ReviewRecord>> {
    let sql = format!("SELECT {REVIEW_COLUMNS} FROM reviews WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id.as_str()], map_review)
        .optional()?)
}

/// 结案（接受或拒绝）。只写审核状态，不触碰知识对象本身——
/// 真正的状态迁移由 `claim_relation_repository::decide` 负责。
pub fn resolve(conn: &Connection, id: &ReviewId, status: ReviewStatus) -> AppResult<()> {
    if status == ReviewStatus::Pending {
        return Err(crate::error::AppError::Invalid(
            "resolve 只接受 accepted / rejected / superseded".into(),
        ));
    }
    let affected = conn.execute(
        "UPDATE reviews SET status = ?1, reviewed_at = datetime('now') WHERE id = ?2",
        params![status.as_str(), id.as_str()],
    )?;
    if affected == 0 {
        return Err(crate::error::AppError::NotFound(format!(
            "审核记录 {id} 不存在"
        )));
    }
    Ok(())
}

/// 待审数量。
pub fn count_pending(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM reviews WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    #[test]
    fn pending_records_round_trip() {
        let conn = memory_db();
        let id = insert_pending(
            &conn,
            ReviewTarget::AgentProposal,
            "claim:123",
            serde_json::json!({ "reason": "merge entities" }),
        )
        .unwrap();

        let stored = get(&conn, &id).unwrap().unwrap();
        assert_eq!(stored.target_type, ReviewTarget::AgentProposal);
        assert_eq!(stored.target_id, "claim:123");
        assert_eq!(stored.status, ReviewStatus::Pending);
        assert_eq!(stored.proposal["reason"], "merge entities");
        assert_eq!(count_pending(&conn).unwrap(), 1);
    }

    #[test]
    fn resolving_closes_the_record_and_is_reported_when_missing() {
        let conn = memory_db();
        let id = insert_pending(
            &conn,
            ReviewTarget::ResearchFinding,
            "task:1",
            serde_json::json!({}),
        )
        .unwrap();

        resolve(&conn, &id, ReviewStatus::Accepted).unwrap();
        assert_eq!(count_pending(&conn).unwrap(), 0);
        assert!(list_pending(&conn, 10).unwrap().is_empty());

        let missing = resolve(&conn, &ReviewId::from_raw("nope"), ReviewStatus::Rejected)
            .unwrap_err();
        assert_eq!(missing.code(), "NOT_FOUND");
    }

    #[test]
    fn resolving_back_to_pending_is_rejected() {
        let conn = memory_db();
        let id = insert_pending(
            &conn,
            ReviewTarget::Entity,
            "entity:1",
            serde_json::json!({}),
        )
        .unwrap();
        assert!(resolve(&conn, &id, ReviewStatus::Pending).is_err());
    }

    #[test]
    fn proposals_are_listed_newest_first() {
        let conn = memory_db();
        insert_pending(&conn, ReviewTarget::Entity, "a", serde_json::json!({})).unwrap();
        insert_pending(&conn, ReviewTarget::Entity, "b", serde_json::json!({})).unwrap();
        let pending = list_pending(&conn, 10).unwrap();
        assert_eq!(pending.len(), 2);
        // created_at 在同一秒内可能相同，因此只断言集合内容
        let ids: std::collections::HashSet<&str> =
            pending.iter().map(|r| r.target_id.as_str()).collect();
        assert!(ids.contains("a") && ids.contains("b"));
    }
}
