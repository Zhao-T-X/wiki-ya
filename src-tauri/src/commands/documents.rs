//! Document（Inbox）相关命令。
//!
//! 命令层只做三件事：反序列化 → 调用 service → 序列化（TDD §56）。
//! 这里没有事务、没有校验、没有状态判断。

use tauri::State;

use crate::application::capture_service;
use crate::application::dto::{
    CreateDocumentInput, DocumentDetail, DocumentSummary, IdInput, ListDocumentsInput,
};
use crate::domain::common::ids::DocumentId;
use crate::error::AppError;
use crate::AppState;

/// 捕获一份文档：存原文 + 确定性切分（不调用任何 AI）。
#[tauri::command]
pub fn create_document(
    state: State<'_, AppState>,
    input: CreateDocumentInput,
) -> Result<DocumentSummary, AppError> {
    let mut conn = state.open()?;
    capture_service::create_document(&mut conn, input)
}

/// 文档列表。
#[tauri::command]
pub fn list_documents(
    state: State<'_, AppState>,
    input: ListDocumentsInput,
) -> Result<Vec<DocumentSummary>, AppError> {
    let conn = state.open()?;
    capture_service::list_documents(&conn, input.query.as_deref(), input.limit.unwrap_or(100))
}

/// 文档详情：原文 + 切片 + 由它贡献的知识。
#[tauri::command]
pub fn get_document(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<DocumentDetail, AppError> {
    let conn = state.open()?;
    capture_service::get_document(&conn, &DocumentId::from_raw(input.id.trim()))
}

/// 重建切片（原文不变）。
#[tauri::command]
pub fn reindex_document(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<DocumentSummary, AppError> {
    let mut conn = state.open()?;
    capture_service::reindex_document(&mut conn, &DocumentId::from_raw(input.id.trim()))
}
