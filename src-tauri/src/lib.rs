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

/// API Key 主密钥文件路径：与数据库同目录（`<app_data_dir>/secret.key`）。
///
/// 注意：主密钥与数据库同目录，因此本方案能防「数据库被单独复制」，但**不能**
/// 防能读取整个用户目录的本地攻击者——详见 `infrastructure::secrets` 的说明。
fn key_file_path(db_path: &std::path::Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(infrastructure::secrets::KEY_FILE_NAME)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志先行初始化：后续所有模块都能按 WIKIYA_LOG 输出。
    logging::init_from_env();

    tauri::Builder::default()
        .setup(|app| {
            let db_path = resolve_db_path(app.handle())?;

            // 建表 + 应用迁移（按版本增量，幂等）。
            infrastructure::db::initialize(&db_path)?;

            // EXTRACTION-001：把"上次还在跑"的抽取 Run 标记为 interrupted，
            // 不假装还在运行（用户重开应用后能看到诚实的终态）。
            {
                let conn = infrastructure::db::open(&db_path)?;
                let recovered = crate::application::extraction_service::recover_interrupted_runs(&conn)?;
                if recovered > 0 {
                    crate::log_warn!(
                        "启动时将 {} 条未完成的抽取 Run 标记为 interrupted",
                        recovered
                    );
                }
            }

            // SEC-001：初始化 API Key 的加解密器（主密钥文件首次运行自动生成，权限 0600）。
            // 失败不阻断启动：AI 会如实显示为「未启用」，而不是让应用起不来。
            {
                let key_path = key_file_path(&db_path);
                match infrastructure::secrets::SecretCipher::load_or_create(&key_path) {
                    Ok(cipher) => infrastructure::secrets::install_cipher(cipher),
                    Err(err) => {
                        crate::log_warn!("初始化 API Key 加密器失败，AI 将不可用：{err}");
                    }
                }
            }

            // SEC-002：把历史遗留的明文 API Key 加密迁移进 SQLite。
            // 失败不阻断启动（`migrate_legacy_api_key` 内部只记警告、保留原值）。
            {
                let conn = infrastructure::db::open(&db_path)?;
                let _ = infrastructure::secrets::migrate_legacy_api_key(&conn);
            }

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
            // ---- extraction run（EXTRACTION-001：异步抽取后台任务）----
            commands::extraction::start_extraction,
            commands::extraction::get_extraction_run,
            commands::extraction::list_extraction_runs,
            commands::extraction::cancel_extraction,
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
