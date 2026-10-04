//! 候选知识命令（M6）。决策只能由人类发起——不经 Agent/Skill 任何路径。

use tauri::State;

use crate::application::candidate_service;
use crate::application::dto::{CandidateDto, CandidatePageDto, DecideCandidateInput, ListCandidatesInput};
use crate::error::AppError;
use crate::AppState;

/// 按 Run **分页**列出候选（PERF-04：游标分页，首屏只 50 条）。
#[tauri::command]
pub fn list_candidates(
    state: State<'_, AppState>,
    input: ListCandidatesInput,
) -> Result<CandidatePageDto, AppError> {
    let conn = state.open()?;
    candidate_service::list_page(&conn, &input)
}

/// 用户决策一条候选：accept → 落库为 Claim + 演化分析；reject → 留痕。
#[tauri::command]
pub fn decide_candidate(
    state: State<'_, AppState>,
    input: DecideCandidateInput,
) -> Result<CandidateDto, AppError> {
    let mut conn = state.open()?;
    candidate_service::decide(&mut conn, input)
}
