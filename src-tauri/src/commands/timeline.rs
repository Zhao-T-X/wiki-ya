//! Timeline 相关命令（Phase 4 收尾）。

use tauri::State;

use crate::application::dto::TimelineItem;
use crate::application::timeline_service;
use crate::error::AppError;
use crate::AppState;

/// 最近的时间轴事件（Document / Claim / Relation / Research 聚合）。
#[tauri::command]
pub fn list_timeline(
    state: State<'_, AppState>,
    input: Option<serde_json::Value>,
) -> Result<Vec<TimelineItem>, AppError> {
    let _ = input;
    let conn = state.open()?;
    Ok(timeline_service::list(&conn, 100)?)
}
