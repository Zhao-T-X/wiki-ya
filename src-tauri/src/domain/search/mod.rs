//! Search —— 检索语义。
//!
//! PRD §23 的四路召回（FTS5 / Vector / Ontology / Graph）在应用层汇合，
//! 而「怎么汇合」是领域规则，不是 SQL 细节，所以融合算法放在这里：
//! **RRF 与预算是领域概念，换了存储它也不会变。**

pub mod search;

pub use search::{
    rrf_fuse, RetrievalBudget, SearchHitKind, SearchMethod, RankedItem, RRF_K,
};
