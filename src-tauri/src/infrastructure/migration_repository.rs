//! 存量库迁移的持久化查询（Phase 8，TDD §71/§72）。
//!
//! 源库是**外部 schema**（只读探测），目标库的存在性检查在这里做——
//! migration_service 只做编排，不拼 SQL。

use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

/// 列出源库的全部用户表名。
pub fn sqlite_tables(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 表是否存在（源库探测；表名由白名单传入，无注入面）。
pub fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
        params![table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// 列是否存在。
pub fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
        ),
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// 表行数。
pub fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap_or(0)
}

/// 源库的文档行（title/content 必需；content_hash 列可选）。
pub fn source_documents(conn: &Connection) -> AppResult<Vec<(String, String, String, Option<String>)>> {
    let has_hash = column_exists(conn, "documents", "content_hash");
    let sql = if has_hash {
        "SELECT id, title, content, content_hash FROM documents ORDER BY rowid"
    } else {
        "SELECT id, title, content, NULL FROM documents ORDER BY rowid"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 源库的 claim 行（subject/object 名称经实体表联出；无实体表则报错）。
///
/// 返回：(subject_name, predicate, object_name, object_text, content, source_document_id)。
#[allow(clippy::type_complexity)]
pub fn source_claims(
    conn: &Connection,
) -> AppResult<Vec<(String, String, Option<String>, Option<String>, Option<String>, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT e.name, c.predicate, o.name, c.object_text, c.content, c.document_id \
         FROM claims c \
         JOIN entities e ON e.id = c.subject_id \
         LEFT JOIN entities o ON o.id = c.object_id \
         ORDER BY c.rowid",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 目标库是否已有同 hash 的文档（幂等：重复导入在源头拦截，INV-02）。
pub fn document_exists_by_hash(conn: &Connection, content_hash: &str) -> AppResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM documents WHERE content_hash = ?1)",
        params![content_hash],
        |row| row.get::<_, i64>(0),
    )? > 0)
}

/// 目标库是否已有同 (subject, predicate, object) 的 claim（迁移幂等检查）。
pub fn claim_exists(
    conn: &Connection,
    subject_name: &str,
    predicate: &str,
    object_display: Option<&str>,
) -> AppResult<bool> {
    Ok(conn
        .query_row(
            "SELECT EXISTS(\
               SELECT 1 FROM claims c \
               JOIN entities e ON e.id = c.subject_id \
               LEFT JOIN entities o ON o.id = c.object_id \
               WHERE e.name = ?1 AND c.predicate = ?2 \
                 AND IFNULL(o.name, c.object_text) IS ?3)",
            params![subject_name, predicate, object_display],
            |row| row.get::<_, i64>(0),
        )?
        > 0)
}

/// 打开只读源库连接。
pub fn open_source(path: &std::path::Path) -> AppResult<Connection> {
    Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|err| AppError::Internal(format!("无法打开源库 {path:?}：{err}")))
}
