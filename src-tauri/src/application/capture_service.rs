//! Capture —— 知识进入系统的入口。
//!
//! 这是 Phase 1 唯一必须"完全跑通"的写路径，因此它**不依赖任何 AI**：
//! 存原文 + 确定性切分，两步都在一个事务里完成。Local-first 的承诺
//! （PRD §44「无 API Key 时仍可 Capture」）就是由这一点兑现的。
//!
//! 语义抽取属于 Phase 6，届时它是**追加**在 `capture` 之后的一个可选步骤，
//! 而不是替代物——所以这里没有任何"等待 AI"的状态。

use rusqlite::Connection;

use crate::application::dto::{ChunkCard, CreateDocumentInput, DocumentDetail, DocumentSummary};
use crate::domain::common::ids::DocumentId;
use crate::domain::knowledge::chunk::chunk_document;
use crate::domain::knowledge::document::{Document, SourceType};
use crate::error::{AppError, AppResult};
use crate::infrastructure::{claim_repository, document_repository};

/// 解析来源类型。空值取默认，非法值明确报错而不是默默变成 `note`。
fn source_type_from(raw: Option<&str>) -> AppResult<SourceType> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(SourceType::DEFAULT),
        Some(value) => value.to_ascii_lowercase().parse::<SourceType>().map_err(|_| {
            AppError::Domain(format!(
                "未注册的来源类型 {value:?}（可选：{}）",
                SourceType::ALL
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }),
    }
}

fn to_summary(row: &document_repository::DocumentSummaryRow) -> DocumentSummary {
    DocumentSummary {
        id: row.document.id.as_str().to_string(),
        title: row.document.title.clone(),
        source_type: row.document.source_type.as_str().to_string(),
        source_uri: row.document.source_uri.clone(),
        content_hash: row.document.content_hash.clone(),
        chunk_count: row.chunk_count,
        char_count: row.document.content.chars().count() as i64,
        created_at: row.document.created_at.clone(),
        updated_at: row.document.updated_at.clone(),
    }
}

/// 捕获一份文档：写原文 + 切分。
///
/// 幂等由 `content_hash` 的 UNIQUE 约束保证（INV-02）。这里先做一次预检查
/// 只是为了给出可读的错误信息；真正的保证在数据库层。
pub fn create_document(
    conn: &mut Connection,
    input: CreateDocumentInput,
) -> AppResult<DocumentSummary> {
    let (title, content) = Document::validate(&input.title, &input.content)?;
    let source_type = source_type_from(input.source_type.as_deref())?;
    let content_hash = Document::content_hash(&content);

    if document_repository::find_id_by_content_hash(conn, &content_hash)?.is_some() {
        return Err(AppError::Conflict(
            "知识库中已存在内容完全相同的文档（导入是幂等的，不会重复入库）".into(),
        ));
    }

    let document = Document {
        id: DocumentId::new(),
        title,
        content,
        content_hash,
        source_type,
        source_uri: input.source_uri.filter(|value| !value.trim().is_empty()),
        metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        created_at: String::new(),
        updated_at: String::new(),
    };

    // 切分是纯计算，放在事务外：事务里只做 I/O，缩短持锁时间。
    let drafts = chunk_document(&document.content);

    let transaction = conn.transaction()?;
    document_repository::insert(&transaction, &document)?;
    document_repository::replace_chunks(&transaction, &document.id, &drafts)?;
    transaction.commit()?;

    let stored = document_repository::find_by_id(conn, &document.id)?
        .ok_or_else(|| AppError::Internal("文档刚写入却读不到".into()))?;
    let chunk_count = document_repository::list_chunks(conn, &document.id)?.len() as i64;

    Ok(to_summary(&document_repository::DocumentSummaryRow {
        document: stored,
        chunk_count,
    }))
}

/// 文档列表（最近更新在前），可按标题/正文关键词过滤。
pub fn list_documents(
    conn: &Connection,
    query: Option<&str>,
    limit: usize,
) -> AppResult<Vec<DocumentSummary>> {
    let rows = document_repository::list(conn, query, limit)?;
    Ok(rows.iter().map(to_summary).collect())
}

/// 文档详情：原文 + 切片 + 由它贡献的知识。
///
/// `claims` 能让用户直接看到"这段文字被理解成了什么"——
/// 这是 Inbox 与 Knowledge 之间的桥。
pub fn get_document(conn: &Connection, id: &DocumentId) -> AppResult<DocumentDetail> {
    let document = document_repository::find_by_id(conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("文档 {id} 不存在")))?;

    let chunks: Vec<ChunkCard> = document_repository::list_chunks(conn, id)?
        .into_iter()
        .map(|chunk| ChunkCard {
            id: chunk.id.as_str().to_string(),
            document_id: chunk.document_id.as_str().to_string(),
            chunk_index: chunk.chunk_index as i64,
            start_offset: chunk.start_offset as i64,
            end_offset: chunk.end_offset as i64,
            content: chunk.content,
            char_count: chunk.char_count as i64,
        })
        .collect();

    let claims = super::knowledge_service::to_claim_cards_with_lifecycle(
        conn,
        &claim_repository::list_by_document(conn, id)?,
    )?;

    let summary = DocumentSummary {
        id: document.id.as_str().to_string(),
        title: document.title.clone(),
        source_type: document.source_type.as_str().to_string(),
        source_uri: document.source_uri.clone(),
        content_hash: document.content_hash.clone(),
        chunk_count: chunks.len() as i64,
        char_count: document.content.chars().count() as i64,
        created_at: document.created_at.clone(),
        updated_at: document.updated_at.clone(),
    };

    Ok(DocumentDetail {
        document: summary,
        content: document.content,
        chunks,
        claims,
    })
}

/// 重建某文档的切片。
///
/// 原文**完全不变**（Rule 1）：切分参数调整、或切片被误删时，
/// 不需要重新导入文档就能恢复结构。
pub fn reindex_document(conn: &mut Connection, id: &DocumentId) -> AppResult<DocumentSummary> {
    let document = document_repository::find_by_id(conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("文档 {id} 不存在")))?;

    let drafts = chunk_document(&document.content);
    let transaction = conn.transaction()?;
    document_repository::replace_chunks(&transaction, id, &drafts)?;
    transaction.commit()?;

    let chunk_count = document_repository::list_chunks(conn, id)?.len() as i64;
    Ok(to_summary(&document_repository::DocumentSummaryRow {
        document,
        chunk_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    fn input(title: &str, content: &str) -> CreateDocumentInput {
        CreateDocumentInput {
            title: title.into(),
            content: content.into(),
            source_type: None,
            source_uri: None,
            metadata: None,
        }
    }

    #[test]
    fn capture_stores_the_document_and_chunks_it_without_any_ai() {
        let mut conn = memory_db();
        let summary = create_document(&mut conn, input("Rust", "第一段。\n\n第二段。")).unwrap();
        assert_eq!(summary.title, "Rust");
        assert_eq!(summary.source_type, "note");
        assert!(summary.chunk_count >= 1);
        assert_eq!(summary.char_count, "第一段。\n\n第二段。".chars().count() as i64);
        assert_eq!(summary.content_hash.len(), 64);
    }

    #[test]
    fn identical_content_is_rejected_as_a_conflict() {
        let mut conn = memory_db();
        create_document(&mut conn, input("a", "same body")).unwrap();
        let err = create_document(&mut conn, input("b", "same body")).unwrap_err();
        assert_eq!(err.code(), "CONFLICT");
        assert!(err.to_string().contains("幂等"));
    }

    #[test]
    fn blank_titles_and_bodies_are_rejected() {
        let mut conn = memory_db();
        assert_eq!(
            create_document(&mut conn, input("  ", "body")).unwrap_err().code(),
            "INVALID_INPUT"
        );
        assert_eq!(
            create_document(&mut conn, input("title", "  ")).unwrap_err().code(),
            "INVALID_INPUT"
        );
    }

    #[test]
    fn unknown_source_types_are_rejected_instead_of_defaulting() {
        let mut conn = memory_db();
        let mut bad = input("t", "c");
        bad.source_type = Some("carrier_pigeon".into());
        assert_eq!(
            create_document(&mut conn, bad).unwrap_err().code(),
            "DOMAIN_RULE_VIOLATION"
        );

        let mut good = input("t2", "c2");
        good.source_type = Some("MARKDOWN".into());
        assert_eq!(create_document(&mut conn, good).unwrap().source_type, "markdown");
    }

    #[test]
    fn listing_filters_by_keyword() {
        let mut conn = memory_db();
        create_document(&mut conn, input("Rust notes", "async fn")).unwrap();
        create_document(&mut conn, input("Python notes", "asyncio")).unwrap();

        assert_eq!(list_documents(&conn, None, 10).unwrap().len(), 2);
        // "Rust" 只出现在 Rust 文档标题里，用来证明关键词过滤真的生效。
        let filtered = list_documents(&conn, Some("Rust"), 10).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Rust notes");
    }

    #[test]
    fn document_detail_exposes_content_chunks_and_claims() {
        let mut conn = memory_db();
        let summary = create_document(&mut conn, input("Note", "第一段。\n\n第二段。")).unwrap();
        let detail = get_document(&conn, &DocumentId::from_raw(&summary.id)).unwrap();

        assert_eq!(detail.content, "第一段。\n\n第二段。");
        assert!(!detail.chunks.is_empty());
        assert!(detail.claims.is_empty(), "Phase 1 还没有抽取，claims 必须为空而不是伪造");
        assert_eq!(detail.document.id, summary.id);
    }

    #[test]
    fn reindex_rebuilds_chunks_without_touching_the_source() {
        let mut conn = memory_db();
        let summary = create_document(&mut conn, input("Note", "段落一。\n\n段落二。")).unwrap();
        let id = DocumentId::from_raw(&summary.id);

        // 人为破坏切片
        conn.execute("DELETE FROM chunks WHERE document_id = ?1", rusqlite::params![summary.id])
            .unwrap();
        assert_eq!(document_repository::list_chunks(&conn, &id).unwrap().len(), 0);

        let rebuilt = reindex_document(&mut conn, &id).unwrap();
        assert!(rebuilt.chunk_count >= 1);
        assert_eq!(rebuilt.content_hash, summary.content_hash, "原文指纹不应改变（Rule 1）");
        assert_eq!(
            get_document(&conn, &id).unwrap().content,
            "段落一。\n\n段落二。"
        );
    }

    #[test]
    fn missing_documents_are_reported_as_not_found() {
        let mut conn = memory_db();
        let id = DocumentId::from_raw("missing");
        assert_eq!(get_document(&conn, &id).unwrap_err().code(), "NOT_FOUND");
        assert_eq!(reindex_document(&mut conn, &id).unwrap_err().code(), "NOT_FOUND");
    }
}
