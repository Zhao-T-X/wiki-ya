//! AI Runtime（Phase 5/6 落地）。
//!
//! 计划结构（TDD §33/§49）：
//!
//! ```text
//! ai/
//! ├── config.rs       环境变量配置（base url / model / key / embedding）
//! ├── provider.rs     Provider trait + Offline / OpenAI 兼容实现
//! ├── context/        Context Efficiency Engine（§33）
//! ├── agents.rs       Agent 角色与系统提示（§52）
//! ├── tools.rs        工具白名单（§51，禁止 execute_sql）
//! ├── runtime.rs      AgentScope Rust 适配（Phase 6 后续）
//! └── extraction.rs   Claim 抽取编排（见 application/ai_service.rs）
//! ```
//!
//! 硬约束：AI 只能调用 `application` 层的用例，
//! 永不直接访问 Repository（TDD §50、Rule 7）。抽取编排放在 `application/ai_service.rs`，
//! 本模块只提供 Provider 抽象、配置、上下文编排与 Agent/工具骨架。

pub mod agents;
pub mod config;
pub mod context;
pub mod provider;
pub mod runtime;
pub mod tools;
