//! AI 抽取相关命令（Phase 5）。
//!
//! 每个 command 只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::ai_service;
use crate::application::dto::{ExtractionReport, IdInput};
use crate::error::AppError;
use crate::AppState;

/// 从一篇文档抽取 Claim 候选（预览，不落库）。
///
/// 未配置 `WIKIYA_API_KEY` 时返回 `enabled: false` 与说明，UI 如实展示，
/// 不伪造任何抽取结果。
#[tauri::command]
pub fn extract_claims(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<ExtractionReport, AppError> {
    let conn = state.open()?;
    ai_service::extract_claims(&conn, &input.id)
}
