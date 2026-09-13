//! wiki-ya 应用入口与依赖装配。
//!
//! 依赖方向（TDD §86，不可违反）：
//!
//! ```text
//! UI  → Commands  → Application → Domain
//!                                  ↑
//!          Infrastructure ─────────┘   （只实现 Domain 定义的 Repository trait）
//!
//! AI Runtime → Application → Domain
//! ```
//!
//! 禁止：Domain → Tauri / Domain → SQLite / Agent → SQL。

pub mod ai;
pub mod application;
pub mod commands;
pub mod domain;
pub mod error;
pub mod events;
pub mod infrastructure;
pub mod logging;

use std::path::PathBuf;

use tauri::Manager;

use crate::error::{AppError, AppResult};

/// 全局应用状态。
///
/// 只持有数据库路径，不持有长连接：本地单用户场景下每次操作开连接、
/// 依赖 WAL 并发（与参考实现一致），避免连接被 mutex 串行化成为瓶颈。
#[derive(Debug, Clone)]
pub struct AppState {
    pub db_path: PathBuf,
}

impl AppState {
    /// 打开一个已应用迁移的数据库连接。
    ///
    /// 所有 Repository 都从这里取连接，保证 `PRAGMA`（外键、WAL、
    /// busy_timeout）在任何调用路径下都已生效。
    pub fn open(&self) -> AppResult<rusqlite::Connection> {
        infrastructure::db::open(&self.db_path)
    }
}

/// 解析并准备数据库文件路径：`<app_data_dir>/wikiya.db`。
fn resolve_db_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("无法定位应用数据目录：{e}")))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("wikiya.db"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志先行初始化：后续所有模块都能按 WIKIYA_LOG 输出。
    logging::init_from_env();

    tauri::Builder::default()
        .setup(|app| {
            let db_path = resolve_db_path(app.handle())?;

            // 建表 + 应用迁移（幂等，每次启动都跑）。
            infrastructure::db::initialize(&db_path)?;
            app.manage(AppState { db_path });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // ---- settings / 元信息 ----
            commands::settings::app_info,
            commands::settings::list_registries,
            commands::settings::knowledge_health,
            commands::settings::get_settings,
            commands::settings::update_settings,
            // ---- ai（Phase 5 抽取）----
            commands::ai::extract_claims,
            // ---- ask（Phase 6 问答）----
            commands::ask::ask,
            // ---- research（Phase 6 多步研究）----
            commands::research::start_research,
            commands::research::list_research_tasks,
            // ---- timeline（Phase 4 收尾）----
            commands::timeline::list_timeline,
            // ---- migration（Phase 8）----
            commands::migration::probe_migration,
            commands::migration::run_migration,
            // ---- documents（Inbox / Document）----
            commands::documents::create_document,
            commands::documents::list_documents,
            commands::documents::get_document,
            commands::documents::reindex_document,
            // ---- knowledge（Entity / Claim / Evidence）----
            commands::knowledge::list_entities,
            commands::knowledge::get_entity,
            commands::knowledge::list_claims,
            commands::knowledge::get_claim,
            commands::knowledge::get_claim_history,
            commands::knowledge::create_claim,
            commands::knowledge::list_evidence,
            // ---- search ----
            commands::search::search,
            // ---- evolution / review ----
            commands::review::analyze_document,
            commands::review::list_review_items,
            commands::review::decide_claim_relation,
        ])
        .run(tauri::generate_context!())
        .expect("wiki-ya 启动失败");
}
