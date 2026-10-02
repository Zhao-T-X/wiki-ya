//! 候选知识命令（M6）。决策只能由人类发起——不经 Agent/Skill 任何路径。

use tauri::State;

use crate::application::candidate_service;
use crate::application::dto::{CandidateDto, DecideCandidateInput, IdInput};
use crate::error::AppError;
use crate::AppState;

/// 按 Run 列出候选（产生顺序）。
#[tauri::command]
pub fn list_candidates(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<Vec<CandidateDto>, AppError> {
    let conn = state.open()?;
    candidate_service::list_by_run(&conn, &input.id)
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
