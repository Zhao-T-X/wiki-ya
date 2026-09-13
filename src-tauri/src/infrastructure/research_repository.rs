//! Research 任务的持久化查询（Phase 6 收尾：历史列表）。
//!
//! research_tasks 表只写不读的现状在这里结束：历史列表是唯一的读路径，
//! findings 里的 JSON 摘要在这里解析成可展示文本。

use rusqlite::{params, Connection};

use crate::error::AppResult;

/// 一条研究任务的历史记录（供列表展示）。
pub struct ResearchTaskRow {
    pub id: String,
    pub question: String,
    pub status: String,
    /// findings.summary 的前缀（解析失败或缺失时为 `None`）。
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 最近的研究任务（新→旧）。
pub fn list_tasks(conn: &Connection, limit: usize) -> AppResult<Vec<ResearchTaskRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, question_text, status, findings, created_at, updated_at \
         FROM research_tasks ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, question, status, findings, created_at, updated_at) = row?;
        let summary = findings.and_then(|text| {
            serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .get("summary")
                        .and_then(|s| s.as_str())
                        .map(|s| s.chars().take(200).collect())
                })
        });
        out.push(ResearchTaskRow {
            id,
            question,
            status,
            summary,
            created_at,
            updated_at,
        });
    }
    Ok(out)
}
