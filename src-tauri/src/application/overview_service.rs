//! 首页概览聚合（PERF-06）。
//!
//! 首页冷加载原本要发 5 次 IPC（app_info / list_registries / list_documents /
//! list_review_items / list_extraction_runs）。每次 IPC 在当前架构下都要
//! `state.open()` 开一条 SQLite 连接、走一遍连接级 PRAGMA——单用户本地应用里，
//! 这条链路是首页加载耗时的主要构成（AI 调用才是大头，但那部分本就在后台）。
//!
//! 这里把 5 次往返压成 1 次：**同一条连接**上按固定顺序取齐各段数据。
//!
//! 诚实边界：各段的条数上限沿用原调用点（文档 8 / 待审 5 / Run 10），
//! 不为了"聚合"而改变任何语义；任一段失败则整体失败（不返回半截数据）。

use rusqlite::Connection;

use crate::application::capture_service;
use crate::application::dto::HomeOverview;
use crate::application::extraction_service;
use crate::application::review_service;
use crate::application::settings_service;
use crate::error::AppResult;
use std::path::Path;

/// 首页各段的条数上限（与原调用点一致）。
const DOC_LIMIT: usize = 8;
const REVIEW_LIMIT: usize = 5;
const RUN_LIMIT: usize = 10;

/// 一次取齐首页概览。
pub fn home_overview(conn: &Connection, db_path: &Path) -> AppResult<HomeOverview> {
    Ok(HomeOverview {
        app_info: settings_service::app_info(db_path, conn)?,
        registries: settings_service::list_registries()?,
        documents: capture_service::list_documents(conn, None, DOC_LIMIT)?,
        pending_review: review_service::list_review_items(conn, REVIEW_LIMIT)?,
        recent_runs: extraction_service::list_runs(conn, RUN_LIMIT)?,
    })
}
