//! Domain Core —— 系统的真相所在。
//!
//! 本层不知道 Tauri、不知道 SQLite、不知道 LLM（TDD §86）。
//! 它只表达：什么是知识、知识如何演化、什么算合法状态迁移。
//!
//! 子域：
//! - [`ontology`]  Entity / EntityType / Predicate / Relation / Event / 注册表 / 归一化 / 消解
//! - [`knowledge`] Claim / Document / Chunk / Idea / Question
//! - [`evidence`]  Claim ↔ Source 的桥与证据分级
//! - [`evolution`] 演化分类、冲突引擎、时间逻辑
//! - [`graph`]     Entity 邻域与图查询语义
//! - [`review`]    提案审核
//! - [`search`]    检索语义（RRF、预算）

pub mod common;
pub mod extraction;
pub mod evidence;
pub mod evolution;
pub mod graph;
pub mod knowledge;
pub mod ontology;
pub mod review;
pub mod search;
