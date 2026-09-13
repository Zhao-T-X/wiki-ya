//! Evolution —— 知识如何随时间演进而不被覆盖。
//!
//! Rule 2：**Claim is append/evolve, never silently overwrite.**
//!
//! 模块分工：
//! - [`rules`]      确定性判定的静态规则（单值谓语、优先级、自动化白名单）
//! - [`classifier`] 演化关系的枚举与映射
//! - [`conflict`]   `compare_claim`：两条 Claim 之间的关系判定
//! - [`temporal`]   「当前知识」的派生
//! - [`engine`]     对外入口：分类与批量分析
//!
//! 一条贯穿全部实现的原则：**确定性优先的判定永不自行宣布「取代」**。
//! 判断"新值替换了旧值"需要时间或文本证据，只有 LLM 建议或人工确认
//! 才能产生 `supersedes`。假装的确定性比诚实的问题更糟。

pub mod classifier;
pub mod conflict;
pub mod engine;
pub mod rules;
pub mod temporal;

pub use classifier::{ClaimRelationStatus, ClaimRelationType, EvolutionClassification};
pub use conflict::{compare_claim, ClaimView, SuggestedAction, Verdict};
pub use engine::{analyze, classify};
