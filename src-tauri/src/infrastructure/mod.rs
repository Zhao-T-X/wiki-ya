//! Infrastructure —— SQLite / FTS5 / 文件 / 缓存的具体实现。
//!
//! 约束：Repository 只实现 Domain 定义的 trait，**Domain 不知道 SQLite**。
//! 业务层不允许拼 SQL（TDD §12）。

pub mod claim_relation_repository;
pub mod claim_repository;
pub mod db;
pub mod document_repository;
pub mod embedding_repository;
pub mod extraction_run_repository;
pub mod migration_repository;
pub mod research_repository;
pub mod secrets;
pub mod settings_repository;
pub mod telemetry_repository;
pub mod timeline_repository;
pub mod entity_repository;
pub mod evidence_repository;
pub mod fts_repository;
pub mod relation_repository;
pub mod review_repository;
