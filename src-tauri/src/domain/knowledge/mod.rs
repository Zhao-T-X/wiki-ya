//! Knowledge —— 知识对象本身。
//!
//! - [`document`] 原始文档（Source of Truth，不可被结构化知识覆盖）
//! - [`chunk`]    文档的计算单位（切分是确定性的，不依赖 AI）
//! - [`claim`]    知识的核心单位
//! - [`idea`]     尚未成为事实的思考
//! - [`question`] 未解决的问题

pub mod chunk;
pub mod claim;
pub mod document;
pub mod idea;
pub mod question;
