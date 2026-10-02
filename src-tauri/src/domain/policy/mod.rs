//! Policy —— 权限边界（M5）。
//!
//! 三级权限，层级递进（高含低）：
//!
//! ```text
//! READ ⊆ PROPOSE ⊆ MUTATE
//! ```
//!
//! - **READ**：检索、问答、对比——只读知识库；
//! - **PROPOSE**：产出候选 / 提案（进入 Review 队列），不改 Current；
//! - **MUTATE**：Accept / Supersede / Change Current / Rollback——**仅人类**
//!   可触发（Review 决策命令是唯一入口，Agent/Skill 无任何可达路径）。
//!
//! 强制点：
//! 1. Tool 分发层（`ai/tools.rs::execute`）：每个工具声明所需权限，
//!    调用方 policy 不足即拒绝；
//! 2. Skill 编排层（`agent_profile_service`）：Skill 需求 ⊆ Profile 上限；
//! 3. 结构性保证（测试固化）：工具白名单里不存在 MUTATE 级工具——
//!    「AI 不拥有写权限」不是口号，是枚举级别的约束。

use std::str::FromStr;

use crate::string_enum;

string_enum! {
    /// 权限级别（高含低：mutate ⊇ propose ⊇ read）。
    pub enum Policy {
        Read => "read",
        Propose => "propose",
        Mutate => "mutate",
    }
}

impl Policy {
    /// 权限等级（只读访问器：1=read，2=propose，3=mutate）。
    pub fn rank_of(&self) -> u8 {
        match self {
            Policy::Read => 1,
            Policy::Propose => 2,
            Policy::Mutate => 3,
        }
    }

    /// 当前权限是否满足所需权限（层级判定）。
    pub fn at_least(&self, required: &Policy) -> bool {
        self.rank_of() >= required.rank_of()
    }

    /// 从列表中取最高权限（Profile policy 存储兼容：`["read","propose"]`
    /// 的上限是 propose）。
    pub fn highest_of<'a>(items: impl Iterator<Item = &'a str>) -> Option<Policy> {
        items
            .filter_map(|item| Policy::from_str(item).ok())
            .max_by_key(|p| p.rank_of())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_is_monotonic() {
        assert!(Policy::Read.at_least(&Policy::Read));
        assert!(!Policy::Read.at_least(&Policy::Propose));
        assert!(Policy::Propose.at_least(&Policy::Read));
        assert!(Policy::Propose.at_least(&Policy::Propose));
        assert!(!Policy::Propose.at_least(&Policy::Mutate));
        assert!(Policy::Mutate.at_least(&Policy::Read));
        assert!(Policy::Mutate.at_least(&Policy::Mutate));
    }

    #[test]
    fn highest_of_picks_the_cap() {
        assert_eq!(
            Policy::highest_of(["read", "propose"].into_iter()),
            Some(Policy::Propose)
        );
        assert_eq!(Policy::highest_of(["read"].into_iter()), Some(Policy::Read));
        assert_eq!(Policy::highest_of([].into_iter()), None);
    }
}
