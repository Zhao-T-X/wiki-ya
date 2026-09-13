//! 检索命令。

use tauri::State;

use crate::application::dto::{SearchInput, SearchResponse};
use crate::application::search_service;
use crate::error::AppError;
use crate::AppState;

/// 统一检索入口（Phase 1 为词法检索，语义检索降级并如实说明）。
#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    input: SearchInput,
) -> Result<SearchResponse, AppError> {
    let conn = state.open()?;
    search_service::search(&conn, input)
}
