//! 存量库迁移命令（Phase 8）。

use std::path::Path;

use tauri::State;

use crate::application::dto::{MigrationInput, MigrationProbe, MigrationReport};
use crate::application::migration_service;
use crate::error::AppError;
use crate::AppState;

/// 只读探测存量库：报告可迁移的表与数量。
#[tauri::command]
pub fn probe_migration(
    state: State<'_, AppState>,
    input: MigrationInput,
) -> Result<MigrationProbe, AppError> {
    let _ = state;
    migration_service::probe(Path::new(input.source_path.trim()))
}

/// 备份当前库并从存量库导入（documents → claims，幂等可重跑）。
#[tauri::command]
pub fn run_migration(
    state: State<'_, AppState>,
    input: MigrationInput,
) -> Result<MigrationReport, AppError> {
    let backup_path = migration_service::backup(&state.db_path)?;
    let mut conn = state.open()?;
    let mut report = migration_service::import(&mut conn, Path::new(input.source_path.trim()))?;
    report.backup_path = Some(backup_path);
    Ok(report)
}
