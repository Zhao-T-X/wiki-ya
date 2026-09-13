//! 演化决策（K-002 / K-003）：把「知识为什么变成 superseded」从存储层
//! 提升为**领域概念**。
//!
//! 之前这一步写在 `claim_relation_repository::decide()` 里，Repository 自己
//! 决定"什么关系该改哪个状态"。问题不在于架构不漂亮，而在于：以后每新增
//! 一条改状态的路径（rollback / batch review / import / AI extraction），
//! 都会各自实现一遍"怎么把 Claim 变成 superseded"。
//!
//! 现在规则收敛到这里唯一的一处纯函数 [`EvolutionTransition::plan`]：
//!
//! ```text
//! ReviewAction + relationship + target 当前状态 + 回滚锚点
//!                     ↓
//!             EvolutionTransition
//! ```
//!
//! Repository 只负责**持久化**这个结果（INV-08：superseded 只能由此产生）。

use crate::domain::evolution::classifier::{ClaimRelationStatus, ClaimRelationType};
use crate::domain::knowledge::claim::ClaimStatus;

/// 审核动作（与存储层解耦：Repository 不再理解"accept/reject/reset"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewAction {
    /// 接受：`supersedes` 在此时才会产生事实效果。
    Accept,
    /// 拒绝：关系作废，不改动任何 Claim。
    Reject,
    /// 撤销：回到待审；若此前确认过取代，则恢复旧状态（K-009）。
    Reset,
}

impl ReviewAction {
    /// 该动作对应的关系行状态。
    pub fn relation_status(self) -> ClaimRelationStatus {
        match self {
            ReviewAction::Accept => ClaimRelationStatus::Accepted,
            ReviewAction::Reject => ClaimRelationStatus::Rejected,
            ReviewAction::Reset => ClaimRelationStatus::Candidate,
        }
    }
}

/// 一次审核决策应当产生的状态迁移（纯数据，无副作用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvolutionTransition {
    /// 关系行要落成的状态。
    pub relation_status: ClaimRelationStatus,
    /// 目标 Claim 要变成的状态；`None` 表示不动它。
    pub target_status: Option<ClaimStatus>,
    /// 需要记录的「回滚锚点」（仅首次 accepted supersedes；避免把 superseded 记成锚点）。
    pub record_previous_status: Option<ClaimStatus>,
}

impl EvolutionTransition {
    /// 由（关系类型, 审核动作, 目标当前状态, 已有回滚锚点）推出迁移。
    ///
    /// 这是**唯一**决定"知识为什么变化"的地方（K-002/K-003）：
    /// - 只有「最终关系是 supersedes 且 accepted」才把 target 置为 superseded；
    /// - 其余一切情况（拒绝 / 撤销 / **改判成别的关系**）都按锚点精确回滚（INV-09）。
    pub fn plan(
        relationship: ClaimRelationType,
        action: ReviewAction,
        current_target_status: Option<ClaimStatus>,
        rollback_anchor: Option<ClaimStatus>,
    ) -> Self {
        let relation_status = action.relation_status();
        let is_accepted_supersedes = relationship == ClaimRelationType::Supersedes
            && relation_status == ClaimRelationStatus::Accepted;

        if is_accepted_supersedes {
            Self {
                relation_status,
                // 目标行存在才会被改动（不存在则无从改起）。
                target_status: current_target_status.map(|_| ClaimStatus::Superseded),
                // 只有目标当前不是 superseded 时才记录锚点，防止重复确认把
                // superseded 存成 previous_status（INV-09 的精确恢复前提）。
                record_previous_status: current_target_status
                    .filter(|status| *status != ClaimStatus::Superseded),
            }
        } else {
            Self {
                relation_status,
                // 精确恢复：没有锚点就什么都不做。
                target_status: rollback_anchor,
                record_previous_status: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERIFIED: Option<ClaimStatus> = Some(ClaimStatus::Verified);

    #[test]
    fn accepted_supersedes_records_the_anchor_and_marks_target() {
        let plan = EvolutionTransition::plan(
            ClaimRelationType::Supersedes,
            ReviewAction::Accept,
            VERIFIED,
            None,
        );
        assert_eq!(plan.relation_status, ClaimRelationStatus::Accepted);
        assert_eq!(plan.target_status, Some(ClaimStatus::Superseded));
        assert_eq!(plan.record_previous_status, Some(ClaimStatus::Verified));
    }

    #[test]
    fn repeated_accept_does_not_overwrite_the_anchor() {
        // 目标已经是 superseded：不能再把它记成锚点。
        let plan = EvolutionTransition::plan(
            ClaimRelationType::Supersedes,
            ReviewAction::Accept,
            Some(ClaimStatus::Superseded),
            Some(ClaimStatus::Verified),
        );
        assert_eq!(plan.target_status, Some(ClaimStatus::Superseded));
        assert_eq!(plan.record_previous_status, None);
    }

    #[test]
    fn reset_restores_from_the_anchor() {
        let plan = EvolutionTransition::plan(
            ClaimRelationType::Supersedes,
            ReviewAction::Reset,
            Some(ClaimStatus::Superseded),
            VERIFIED,
        );
        assert_eq!(plan.relation_status, ClaimRelationStatus::Candidate);
        assert_eq!(plan.target_status, Some(ClaimStatus::Verified), "必须精确恢复");
        assert_eq!(plan.record_previous_status, None);
    }

    #[test]
    fn reclassifying_to_a_non_supersede_relationship_rolls_back() {
        // 改判成 supplements 且 accepted：仍须回滚（这是曾经的真实 bug）。
        let plan = EvolutionTransition::plan(
            ClaimRelationType::Supplements,
            ReviewAction::Accept,
            Some(ClaimStatus::Superseded),
            VERIFIED,
        );
        assert_eq!(plan.relation_status, ClaimRelationStatus::Accepted);
        assert_eq!(plan.target_status, Some(ClaimStatus::Verified));
        assert_eq!(plan.record_previous_status, None);
    }

    #[test]
    fn non_supersede_relations_never_touch_target_state() {
        for relationship in [
            ClaimRelationType::Duplicate,
            ClaimRelationType::Coexists,
            ClaimRelationType::Contradicts,
            ClaimRelationType::Supplements,
            ClaimRelationType::Unclear,
        ] {
            let plan = EvolutionTransition::plan(relationship, ReviewAction::Accept, VERIFIED, None);
            assert_eq!(plan.target_status, None, "{relationship:?} 不应改动 Claim 状态");
            assert_eq!(plan.record_previous_status, None);
        }
    }

    #[test]
    fn reject_never_touches_any_claim() {
        let plan = EvolutionTransition::plan(
            ClaimRelationType::Supersedes,
            ReviewAction::Reject,
            VERIFIED,
            None,
        );
        assert_eq!(plan.relation_status, ClaimRelationStatus::Rejected);
        assert_eq!(plan.target_status, None);
    }
}
