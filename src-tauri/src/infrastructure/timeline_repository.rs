//! Timeline 时间线（Phase 4 收尾）：跨对象聚合的派生只读视图。
//!
//! 把 documents / claims / claim_relations / research_tasks 的时间字段
//! UNION 成统一的时间轴行；解析与展示归 application / UI。

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// 时间轴上的一条事件。
pub struct TimelineRow {
    /// `document` | `claim` | `relation` | `research`。
    pub kind: String,
    /// 对象 id（可下钻）。
    pub id: String,
    pub title: String,
    pub detail: String,
    pub at: String,
}

/// 最近的事件（新→旧）。
pub fn list(conn: &Connection, limit: usize) -> AppResult<Vec<TimelineRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM (\
           SELECT 'document' AS kind, id, title AS title, IFNULL(source_type,'') AS detail, \
                  created_at AS at FROM documents \
           UNION ALL \
           SELECT 'claim', c.id, e.name || ' ' || c.predicate || ' ' || \
                  IFNULL(o.name, IFNULL(c.object_text, '')), IFNULL(c.content, ''), c.created_at \
             FROM claims c \
             JOIN entities e ON e.id = c.subject_id \
             LEFT JOIN entities o ON o.id = c.object_id \
           UNION ALL \
           SELECT 'relation', r.id, r.relationship, IFNULL(r.reason, ''), r.created_at \
             FROM claim_relations r \
           UNION ALL \
           SELECT 'research', t.id, t.question_text, t.status, t.updated_at FROM research_tasks t \
         ) ORDER BY at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(TimelineRow {
            kind: row.get(0)?,
            id: row.get(1)?,
            title: row.get(2)?,
            detail: row.get(3)?,
            at: row.get(4)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
