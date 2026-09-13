//! Evidence —— Claim 与来源之间的桥。
//!
//! Rule 3：**Evidence is first-class.**
//! 知识不是 `Claim`，而是 `Claim + Evidence + Source + Time`。
//! 任何重要知识都必须能回答：「你凭什么认为这是真的？」
//!
//! 本模块同时承载 **Evidence Escalation**（PRD §18）：
//! 默认只加载最便宜的一层（引文），只有出现冲突、低置信度、
//! 条件式断言等信号时才逐层展开到段落乃至整块。
//! 核心原则是：**不需要的信息不进入 Context。**

pub mod evidence;

pub use evidence::{
    choose_level, Evidence, EvidenceLevel, DEFAULT_MAX_LEVEL, LOW_CONFIDENCE,
    SIMILAR_QUOTE_RATIO,
};
