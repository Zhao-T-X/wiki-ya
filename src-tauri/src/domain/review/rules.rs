//! 审核排序规则。
//!
//! 待审核队列的展示顺序必须固定且可解释：需要人立刻拍板的排在前面。
//! 这里给出的顺序与存储层 `claim_relations` 查询里的 `CASE` 保持一致，
//! 两处共享同一套优先级定义（一个排序、一个展示）。

use crate::domain::evolution::classifier::ClaimRelationType;

/// 关系类型 → 审核优先级（数字越小越靠前）。
///
/// - `duplicate`：自动已确认，基本不需要人看，但排在最前便于快速核对
/// - `supersedes`：会改变「当前知识」，最危险，紧随其后
/// - `contradicts`：需要人判断哪个成立
/// - `coexists` / `unclear`：破坏性最低，垫后
pub fn review_priority(relationship: &str) -> u8 {
    match relationship.parse::<ClaimRelationType>() {
        Ok(ClaimRelationType::Duplicate) => 1,
        Ok(ClaimRelationType::Supersedes) => 2,
        Ok(ClaimRelationType::Contradicts) => 3,
        Ok(ClaimRelationType::Coexists) => 4,
        Ok(ClaimRelationType::Supplements) => 4,
        Ok(ClaimRelationType::Unclear) => 5,
        // 非法关系类型：兜底放最后，不要让它悄悄排到最前
        Err(_) => u8::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_duplicate_then_supersedes_then_contradicts() {
        assert!(review_priority("duplicate") < review_priority("supersedes"));
        assert!(review_priority("supersedes") < review_priority("contradicts"));
        assert!(review_priority("contradicts") < review_priority("coexists"));
        assert!(review_priority("coexists") < review_priority("unclear"));
    }

    #[test]
    fn illegal_relationships_sink_to_the_bottom() {
        assert_eq!(review_priority("vibes_with"), u8::MAX);
    }
}
