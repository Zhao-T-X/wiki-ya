//! Document 与 Chunk 的持久化。
//!
//! 注意这里**没有** `update_content` 之外的写入口，也没有"由知识回写原文"
//! 的方法——Rule 1 在 API 层面就要体现出来，而不只是靠调用方自觉。

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::common::ids::{ChunkId, DocumentId};
use crate::domain::knowledge::chunk::{Chunk, ChunkDraft};
use crate::domain::knowledge::document::{Document, SourceType};
use crate::error::AppResult;
use crate::infrastructure::db::{json_col, parse_col};

/// 文档列表项：文档本体 + 切片数（Inbox 主视图需要）。
#[derive(Debug, Clone)]
pub struct DocumentSummaryRow {
    pub document: Document,
    pub chunk_count: i64,
}

const DOCUMENT_COLUMNS: &str =
    "id, title, content, source_type, source_uri, content_hash, metadata_json, created_at, updated_at";

fn map_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: parse_col::<DocumentId>(row, 0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        source_type: parse_col::<SourceType>(row, 3)?,
        source_uri: row.get(4)?,
        content_hash: row.get(5)?,
        metadata: json_col(row, 6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// 插入文档。`content_hash` 冲突会返回 `Conflict`（INV-02）。
pub fn insert(conn: &Connection, document: &Document) -> AppResult<()> {
    conn.execute(
        "INSERT INTO documents(id,title,content,source_type,source_uri,content_hash,metadata_json)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            document.id.as_str(),
            document.title,
            document.content,
            document.source_type.as_str(),
            document.source_uri,
            document.content_hash,
            serde_json::to_string(&document.metadata)?,
        ],
    )?;
    Ok(())
}

/// 按 id 取文档。
pub fn find_by_id(conn: &Connection, id: &DocumentId) -> AppResult<Option<Document>> {
    let sql = format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id.as_str()], map_document)
        .optional()?)
}

/// 按内容指纹查已存在的文档 id（导入幂等的预检查）。
///
/// 即便预检查通过，插入时仍可能因并发而冲突——所以真正的保证是
/// 数据库的 UNIQUE 约束，这里只是为了让错误信息更友好。
pub fn find_id_by_content_hash(
    conn: &Connection,
    content_hash: &str,
) -> AppResult<Option<DocumentId>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT id FROM documents WHERE content_hash = ?1",
            params![content_hash],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.map(DocumentId::from_raw))
}

/// 列出文档（最近更新在前），可选按关键词过滤标题/正文。
pub fn list(
    conn: &Connection,
    query: Option<&str>,
    limit: usize,
) -> AppResult<Vec<DocumentSummaryRow>> {
    let limit = limit.clamp(1, 500) as i64;
    let sql = format!(
        "SELECT d.{},
                (SELECT COUNT(*) FROM chunks c WHERE c.document_id = d.id)
         FROM documents d
         WHERE (?1 IS NULL OR d.title LIKE ?2 OR d.content LIKE ?2)
         ORDER BY d.updated_at DESC
         LIMIT ?3",
        DOCUMENT_COLUMNS.replace(", ", ", d.")
    );

    let pattern = query
        .map(|q| q.trim())
        .filter(|q| !q.is_empty())
        .map(|q| format!("%{q}%"));
    let query_value = pattern.as_deref();

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![query_value, pattern.clone(), limit], |row| {
        Ok(DocumentSummaryRow {
            document: map_document(row)?,
            chunk_count: row.get(9)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 用新的切片集合替换某文档的全部切片。
///
/// 整体替换而不是增量合并：切片是**派生数据**（TDD §85），
/// 重建比"猜测哪些切片该保留"更安全，也天然实现了 `reindex`。
/// 删除旧切片不会影响 Claim —— 证据的 `chunk_id` 会置空（SET NULL），
/// 但引文与偏移仍在 evidence 行里，证据本身不丢。
pub fn replace_chunks(
    conn: &Connection,
    document_id: &DocumentId,
    drafts: &[ChunkDraft],
) -> AppResult<usize> {
    conn.execute(
        "DELETE FROM chunks WHERE document_id = ?1",
        params![document_id.as_str()],
    )?;

    let mut statement = conn.prepare(
        "INSERT INTO chunks(id,document_id,chunk_index,start_offset,end_offset,content,char_count)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
    )?;
    for draft in drafts {
        statement.execute(params![
            ChunkId::new().as_str(),
            document_id.as_str(),
            draft.chunk_index as i64,
            draft.start_offset as i64,
            draft.end_offset as i64,
            draft.content,
            Chunk::char_count_of(&draft.content) as i64,
        ])?;
    }
    Ok(drafts.len())
}

/// 列出某文档的全部切片（按序）。
pub fn list_chunks(conn: &Connection, document_id: &DocumentId) -> AppResult<Vec<Chunk>> {
    let mut statement = conn.prepare(
        "SELECT id,document_id,chunk_index,start_offset,end_offset,content,char_count
         FROM chunks WHERE document_id = ?1 ORDER BY chunk_index",
    )?;
    let rows = statement.query_map(params![document_id.as_str()], |row| {
        Ok(Chunk {
            id: parse_col::<ChunkId>(row, 0)?,
            document_id: parse_col::<DocumentId>(row, 1)?,
            chunk_index: row.get::<_, i64>(2)? as usize,
            start_offset: row.get::<_, i64>(3)? as usize,
            end_offset: row.get::<_, i64>(4)? as usize,
            content: row.get(5)?,
            char_count: row.get::<_, i64>(6)? as usize,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 文档总数。
pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?)
}

/// 切片总数。
pub fn count_chunks(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::chunk::chunk_document;
    use crate::infrastructure::db::tests::memory_db;

    fn document(title: &str, content: &str) -> Document {
        Document {
            id: DocumentId::new(),
            title: title.to_string(),
            content: content.to_string(),
            content_hash: Document::content_hash(content),
            source_type: SourceType::Note,
            source_uri: None,
            metadata: serde_json::json!({}),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn insert_then_read_round_trips_every_field() {
        let conn = memory_db();
        let doc = document("Rust async", "Rust 1.75 stabilized async fn in trait.");
        insert(&conn, &doc).unwrap();

        let loaded = find_by_id(&conn, &doc.id).unwrap().unwrap();
        assert_eq!(loaded.title, "Rust async");
        assert_eq!(loaded.content_hash, doc.content_hash);
        assert_eq!(loaded.source_type, SourceType::Note);
    }

    #[test]
    fn duplicate_content_hash_is_reported_as_a_conflict() {
        let conn = memory_db();
        insert(&conn, &document("a", "same body")).unwrap();
        let err = insert(&conn, &document("b", "same body")).unwrap_err();
        assert_eq!(err.code(), "CONFLICT");
    }

    #[test]
    fn content_hash_lookup_finds_the_existing_document() {
        let conn = memory_db();
        let doc = document("a", "body");
        insert(&conn, &doc).unwrap();
        let found = find_id_by_content_hash(&conn, &doc.content_hash).unwrap();
        assert_eq!(found.map(|id| id.into_string()), Some(doc.id.into_string()));
        assert!(find_id_by_content_hash(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn replace_chunks_rewrites_the_set_and_is_idempotent() {
        let conn = memory_db();
        let doc = document("a", "第一段。\n\n第二段。\n\n第三段。");
        insert(&conn, &doc).unwrap();

        let drafts = chunk_document(&doc.content);
        let written = replace_chunks(&conn, &doc.id, &drafts).unwrap();
        assert_eq!(written, drafts.len());

        // 再次重建：数量不变，不产生重复行
        replace_chunks(&conn, &doc.id, &drafts).unwrap();
        let chunks = list_chunks(&conn, &doc.id).unwrap();
        assert_eq!(chunks.len(), drafts.len());
        for (index, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, index);
            assert_eq!(chunk.char_count, chunk.content.chars().count());
        }
    }

    #[test]
    fn list_filters_by_keyword_and_reports_chunk_counts() {
        let conn = memory_db();
        let rust = document("Rust", "async fn in trait");
        let python = document("Python", "asyncio");
        insert(&conn, &rust).unwrap();
        insert(&conn, &python).unwrap();
        replace_chunks(&conn, &rust.id, &chunk_document(&rust.content)).unwrap();

        let all = list(&conn, None, 10).unwrap();
        assert_eq!(all.len(), 2);

        // "trait" 只出现在 Rust 文档正文里，用来证明关键词过滤真的生效。
        let filtered = list(&conn, Some("trait"), 10).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].document.title, "Rust");
        assert_eq!(filtered[0].chunk_count, 1);

        assert!(list(&conn, Some("   "), 10).unwrap().len() == 2);
    }

    #[test]
    fn deleting_a_document_cascades_to_chunks() {
        let conn = memory_db();
        let doc = document("a", "body");
        insert(&conn, &doc).unwrap();
        replace_chunks(&conn, &doc.id, &chunk_document(&doc.content)).unwrap();
        assert_eq!(count_chunks(&conn).unwrap(), 1);

        conn.execute("DELETE FROM documents WHERE id=?1", params![doc.id.as_str()])
            .unwrap();
        assert_eq!(count_chunks(&conn).unwrap(), 0);
    }

    #[test]
    fn invalid_source_type_in_the_database_surfaces_as_an_error() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash,source_type)
             VALUES('d1','t','c','h','carrier_pigeon')",
            [],
        )
        .unwrap();
        let result = find_by_id(&conn, &DocumentId::from_raw("d1"));
        assert!(result.is_err(), "非法 source_type 必须在读取时暴露");
    }
}
