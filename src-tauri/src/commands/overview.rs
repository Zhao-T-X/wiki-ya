//! 首页概览命令（PERF-06：把 5 次 IPC 压成 1 次）。

use tauri::State;

use crate::application::dto::HomeOverview;
use crate::application::overview_service;
use crate::error::AppError;
use crate::AppState;

/// 首页冷加载所需的全部数据（聚合）。
#[tauri::command]
pub fn get_home_overview(state: State<'_, AppState>) -> Result<HomeOverview, AppError> {
    let conn = state.open()?;
    overview_service::home_overview(&conn, &state.db_path)
}
