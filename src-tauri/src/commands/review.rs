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
///
/// PERF-09：虽是纯本地计算，但要扫全文 chunk × 全部 Claim 做两两比较，
/// 长文档下足以卡住主线程数百毫秒。包进 `spawn_blocking` 以离开主线程。
#[tauri::command]
pub async fn analyze_document(
    state: State<'_, AppState>,
    input: AnalyzeDocumentInput,
) -> Result<AnalysisReport, AppError> {
    let db_path = state.db_path.clone();
    let document_id = input.document_id.trim().to_string();
    state.open()?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = crate::infrastructure::db::open(&db_path)?;
        evolution_service::analyze_document(&mut conn, &DocumentId::from_raw(document_id))
    })
    .await
    .map_err(|err| AppError::Internal(format!("演化分析任务异常终止：{err}")))?
}

/// 待审核队列。
#[tauri::command]
pub async fn list_review_items(
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
pub async fn decide_claim_relation(
    state: State<'_, AppState>,
    input: DecideRelationInput,
) -> Result<ClaimRelationCard, AppError> {
    let mut conn = state.open()?;
    review_service::decide_relation(&mut conn, input)
}
