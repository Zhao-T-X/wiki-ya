//! Idea —— 尚未成为事实的思考。
//!
//! Idea 刻意**不强制转成 Claim**（PRD §15）。把"也许可以试试 X"
//! 强行结构化成一个断言，是这类系统最常见的知识污染方式。

use crate::domain::common::ids::{ChunkId, DocumentId, IdeaId};
use crate::domain::common::Timestamp;
use crate::string_enum;

string_enum! {
    /// Idea 状态（5 个）。
    ///
    /// 取参考实现的取值而非 PRD 的 `active/developing/converted/archived`：
    /// 参考实现已落库、有测试、有迁移路径（决策 D4）。
    pub enum IdeaStatus {
        Candidate => "candidate",
        Accepted => "accepted",
        Implemented => "implemented",
        Rejected => "rejected",
        Archived => "archived",
    }
}

impl IdeaStatus {
    pub const DEFAULT: IdeaStatus = IdeaStatus::Candidate;
}

/// 想法。
#[derive(Debug, Clone)]
pub struct Idea {
    pub id: IdeaId,
    pub content: String,
    pub status: IdeaStatus,
    pub confidence: Option<f32>,
    pub source_document_id: Option<DocumentId>,
    pub source_chunk_id: Option<ChunkId>,
    pub created_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idea_statuses_match_the_reference_implementation() {
        assert_eq!(IdeaStatus::ALL.len(), 5);
        assert_eq!(IdeaStatus::DEFAULT.as_str(), "candidate");
        // PRD 里的 converted 不是当前实现的取值
        assert!(!IdeaStatus::ALL.iter().any(|s| s.as_str() == "converted"));
    }
}
