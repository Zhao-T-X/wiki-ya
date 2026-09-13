//! 演化分析与审核命令。

use tauri::State;

use crate::application::dto::{
    AnalysisReport, AnalyzeDocumentInput, ClaimRelationCard, DecideRelationInput, LimitInput,
    ReviewItem,
};
use crate::application::{evolution_service, review_service};
use crate::domain::common::ids::DocumentId;
use crate::error::AppError;
use crate::AppState;

/// 对一份文档做确定性演化分析（不调用 LLM），把待确认的关系写入审核队列。
#[tauri::command]
pub fn analyze_document(
    state: State<'_, AppState>,
    input: AnalyzeDocumentInput,
) -> Result<AnalysisReport, AppError> {
    let mut conn = state.open()?;
    evolution_service::analyze_document(&mut conn, &DocumentId::from_raw(input.document_id.trim()))
}

/// 待审核队列。
#[tauri::command]
pub fn list_review_items(
    state: State<'_, AppState>,
    input: LimitInput,
) -> Result<Vec<ReviewItem>, AppError> {
    let conn = state.open()?;
    review_service::list_review_items(&conn, input.limit.unwrap_or(100))
}

/// 提交审核决策（`accept` / `reject` / `reset`）。
///
/// 这是 `superseded` 状态的唯一入口（INV-08）。
#[tauri::command]
pub fn decide_claim_relation(
    state: State<'_, AppState>,
    input: DecideRelationInput,
) -> Result<ClaimRelationCard, AppError> {
    let mut conn = state.open()?;
    review_service::decide_relation(&mut conn, input)
}
