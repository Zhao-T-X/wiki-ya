//! Timeline 时间线（Phase 4 收尾）。
//!
//! 把 Document / Claim / Claim Relation / Research Task 的关键时间点
//! 聚合成统一时间轴；SQL 在 `timeline_repository`，这里只做映射。

use rusqlite::Connection;

use crate::application::dto::TimelineItem;
use crate::error::AppResult;
use crate::infrastructure::timeline_repository;

/// 最近的事件（新→旧）。
pub fn list(conn: &Connection, limit: usize) -> AppResult<Vec<TimelineItem>> {
    Ok(timeline_repository::list(conn, limit)?
        .into_iter()
        .map(|row| TimelineItem {
            kind: row.kind,
            id: row.id,
            title: row.title,
            detail: row.detail,
            at: row.at,
        })
        .collect())
}
