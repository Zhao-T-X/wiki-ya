//! Question —— 未解决的问题。
//!
//! Question 是 Research 的入口（PRD §16/§27）：它让"我还不知道什么"
//! 也成为一等知识，而不是只能存在于用户脑子里。

use crate::domain::common::ids::{ChunkId, DocumentId, QuestionId};
use crate::domain::common::Timestamp;
use crate::string_enum;

string_enum! {
    /// Question 状态（6 个）。
    ///
    /// 取参考实现的取值（决策 D4）。注意它保留了
    /// `partially_answered` —— 部分解答是研究过程中的常态，
    /// 只有开/闭两态会迫使系统做出不诚实的判断。
    pub enum QuestionStatus {
        Open => "open",
        Answered => "answered",
        PartiallyAnswered => "partially_answered",
        Resolved => "resolved",
        Rejected => "rejected",
        Archived => "archived",
    }
}

impl QuestionStatus {
    pub const DEFAULT: QuestionStatus = QuestionStatus::Open;

    /// 是否还需要继续研究。
    pub fn is_open_for_research(&self) -> bool {
        matches!(self, QuestionStatus::Open | QuestionStatus::PartiallyAnswered)
    }
}

string_enum! {
    /// 问题类型（5 个）。用于决定 Research 采用哪种检索与综合策略。
    pub enum QuestionType {
        Knowledge => "knowledge",
        Research => "research",
        Design => "design",
        Implementation => "implementation",
        Evaluation => "evaluation",
    }
}

impl QuestionType {
    pub const DEFAULT: QuestionType = QuestionType::Knowledge;
}

/// 问题。
#[derive(Debug, Clone)]
pub struct Question {
    pub id: QuestionId,
    pub content: String,
    pub question_type: QuestionType,
    pub status: QuestionStatus,
    pub source_document_id: Option<DocumentId>,
    pub source_chunk_id: Option<ChunkId>,
    pub created_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_enums_match_the_reference_implementation() {
        assert_eq!(QuestionStatus::ALL.len(), 6);
        assert_eq!(QuestionType::ALL.len(), 5);
    }

    #[test]
    fn partially_answered_questions_stay_open_for_research() {
        assert!(QuestionStatus::Open.is_open_for_research());
        assert!(QuestionStatus::PartiallyAnswered.is_open_for_research());
        assert!(!QuestionStatus::Answered.is_open_for_research());
        assert!(!QuestionStatus::Resolved.is_open_for_research());
    }
}
