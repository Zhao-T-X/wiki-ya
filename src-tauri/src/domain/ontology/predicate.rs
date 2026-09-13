//! 受控谓语。
//!
//! wiki-ya 有**两套**互不相同的谓语注册表，边界由
//! `registries/relation-normalization-rules.json` 裁决，而不是靠约定：
//!
//! - [`ClaimPredicate`]（46 个）：描述"某主体具有某性质/关系"，落在 `claims`
//! - [`RelationPredicate`]（19 个）：描述"实体 → 实体"的稳定长期关系，落在 `relations`
//!
//! 同名谓语（如 `uses`）在两边都存在，此时**能否写成 relation 由归一化规则决定**：
//! `uses` 属于 `context_sensitive_predicates`，只有满足
//! `direct_relation_requirements` 才升级为关系，否则退化为 claim。

use crate::error::{AppError, AppResult};
use crate::string_enum;

string_enum! {
    /// Claim 谓语（46 个，封闭集合）。
    pub enum ClaimPredicate {
        Is => "is",
        DefinedAs => "defined_as",
        ClassifiedAs => "classified_as",
        Contains => "contains",
        Includes => "includes",
        ConsistsOf => "consists_of",
        Uses => "uses",
        Retrieves => "retrieves",
        Accesses => "accesses",
        Provides => "provides",
        Receives => "receives",
        Generates => "generates",
        Produces => "produces",
        Creates => "creates",
        Extracts => "extracts",
        Transforms => "transforms",
        Supports => "supports",
        Enables => "enables",
        Allows => "allows",
        Improves => "improves",
        Reduces => "reduces",
        Increases => "increases",
        Decreases => "decreases",
        Prevents => "prevents",
        Causes => "causes",
        LeadsTo => "leads_to",
        DependsOn => "depends_on",
        Requires => "requires",
        Implements => "implements",
        BasedOn => "based_on",
        DerivedFrom => "derived_from",
        Extends => "extends",
        TrainedOn => "trained_on",
        EvaluatedOn => "evaluated_on",
        TestedOn => "tested_on",
        Studies => "studies",
        Investigates => "investigates",
        Evaluates => "evaluates",
        Analyzes => "analyzes",
        ComparesWith => "compares_with",
        BetterThan => "better_than",
        WorseThan => "worse_than",
        DesignedFor => "designed_for",
        UsedFor => "used_for",
        AppliedTo => "applied_to",
        Addresses => "addresses",
    }
}

string_enum! {
    /// Relation 谓语（19 个，封闭集合）。
    pub enum RelationPredicate {
        SubtypeOf => "subtype_of",
        PartOf => "part_of",
        Uses => "uses",
        DependsOn => "depends_on",
        Develops => "develops",
        Creates => "creates",
        Maintains => "maintains",
        Owns => "owns",
        Implements => "implements",
        BasedOn => "based_on",
        DerivedFrom => "derived_from",
        Extends => "extends",
        TrainedOn => "trained_on",
        EvaluatedOn => "evaluated_on",
        Studies => "studies",
        Evaluates => "evaluates",
        ComparesWith => "compares_with",
        AppliedTo => "applied_to",
        DesignedFor => "designed_for",
    }
}

impl ClaimPredicate {
    /// 把自由文本谓语归一为受控值。
    ///
    /// 先做确定性归一（大小写、分隔符），再要求精确落在注册表上。
    /// **没有"最接近的猜测"这一步**——猜错谓语会污染整个图谱。
    pub fn canonical(raw: &str) -> AppResult<ClaimPredicate> {
        let normalized = normalize_predicate(raw);
        if normalized.is_empty() {
            return Err(AppError::Invalid("claim 谓语不能为空".into()));
        }
        normalized.parse::<ClaimPredicate>().map_err(|_| {
            AppError::Domain(format!(
                "未注册的 claim 谓语: {raw:?}（归一化为 {normalized:?}）"
            ))
        })
    }
}

impl RelationPredicate {
    /// 把自由文本谓语归一为受控值（规则同 [`ClaimPredicate::canonical`]）。
    ///
    /// 注意：返回值只说明"这是注册过的 relation 谓语"，
    /// **不代表可以写成 relation 行**——还要过归一化规则（见 `normalization`）。
    pub fn canonical(raw: &str) -> AppResult<RelationPredicate> {
        let normalized = normalize_predicate(raw);
        if normalized.is_empty() {
            return Err(AppError::Invalid("relation 谓语不能为空".into()));
        }
        normalized.parse::<RelationPredicate>().map_err(|_| {
            AppError::Domain(format!(
                "未注册的 relation 谓语: {raw:?}（归一化为 {normalized:?}）"
            ))
        })
    }
}

/// 谓词的确定性归一化。
///
/// 规则（与参考实现一致，必须逐字保持，否则存量数据对不上）：
/// 小写 → 非 `[a-z0-9_]` 转 `_` → 连续 `_` 折叠 → 去首尾 `_`。
///
/// 例：`"Depends On!"` → `depends_on`，`"based-on"` → `based_on`。
pub fn normalize_predicate(raw: &str) -> String {
    let lowered = raw.trim().to_ascii_lowercase();

    let mut mapped = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            mapped.push(ch);
        } else {
            mapped.push('_');
        }
    }

    // 折叠连续下划线
    let mut collapsed = String::with_capacity(mapped.len());
    let mut previous_underscore = false;
    for ch in mapped.chars() {
        if ch == '_' {
            if previous_underscore {
                continue;
            }
            previous_underscore = true;
        } else {
            previous_underscore = false;
        }
        collapsed.push(ch);
    }

    collapsed.trim_matches('_').to_string()
}

/// 实体名归一化（用于消解的 Exact / Normalized 两级）。
///
/// 与谓语归一化不同：这里只压缩空白并做 Unicode 小写，
/// **不删除标点**——"C++" 和 "C" 是两个不同的实体。
pub fn normalize_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut previous_space = false;
    for ch in raw.trim().chars() {
        if ch.is_whitespace() {
            if previous_space {
                continue;
            }
            previous_space = true;
            out.push(' ');
        } else {
            previous_space = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registries_are_frozen() {
        assert_eq!(ClaimPredicate::ALL.len(), 46);
        assert_eq!(RelationPredicate::ALL.len(), 19);
    }

    #[test]
    fn normalisation_matches_the_reference_implementation() {
        assert_eq!(normalize_predicate("Depends On!"), "depends_on");
        assert_eq!(normalize_predicate("based-on"), "based_on");
        assert_eq!(normalize_predicate("  USES  "), "uses");
        assert_eq!(normalize_predicate("a___b"), "a_b");
        assert_eq!(normalize_predicate("___"), "");
    }

    #[test]
    fn unknown_predicates_are_rejected() {
        assert!(ClaimPredicate::canonical("vibes_with").is_err());
        assert!(RelationPredicate::canonical("is").is_err()); // is 只在 claim 注册表里
        assert_eq!(
            ClaimPredicate::canonical("Used For").unwrap(),
            ClaimPredicate::UsedFor
        );
    }

    #[test]
    fn name_normalisation_keeps_punctuation() {
        assert_eq!(normalize_name("  Apple   Inc. "), "apple inc.");
        assert_eq!(normalize_name("C++"), "c++");
        assert_ne!(normalize_name("C++"), normalize_name("C"));
    }
}
