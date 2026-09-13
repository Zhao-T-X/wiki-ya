//! Application Layer —— 用例编排与事务边界（TDD §21/§69）。
//!
//! 职责：
//! - 组合 Domain 能力完成一个用例
//! - **独占事务边界**：所有知识提交在此层开事务，Domain 与 Repository 只在事务内被调用
//! - 幂等：以 `content_hash` / `proposal_id` / `operation_id` 收敛重复调用（INV-02）
//!
//! 本层可以被 Commands 和 AI Runtime 调用；反过来不行。

pub mod ai_service;
pub mod ask_service;
pub mod capture_service;
pub mod dto;
pub mod evolution_service;
pub mod knowledge_service;
pub mod migration_service;
pub mod research_service;
pub mod review_service;
pub mod retrieval_service;
pub mod search_service;
pub mod settings_service;
pub mod timeline_service;
