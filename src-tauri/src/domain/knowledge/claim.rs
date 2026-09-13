//! Claim —— 知识的核心单位。
//!
//! 一个 Claim 是「主语 + 谓语 + 宾语」在某个极性/语气/时间下的断言。
//! 它**只增不改**（Rule 2）：新知识产生新行，旧行只可能因 `supersedes`
//! 而被标记为 `superseded`，且那一步必须经过人工确认。
//!
//! 关于「当前知识」：它不是一个字段，而是由 `status` + 演化关系 + 时间
//! 共同**派生**出来的（Rule 4）。见 [`ClaimStatus::superseded`] 与
//! [`crate::domain::evolution::temporal`]。

use crate::domain::common::ids::{ClaimId, EntityId};
use crate::domain::common::Timestamp;
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::error::{AppError, AppResult};
use crate::string_enum;

string_enum! {
    /// Claim 的生命周期状态（6 个）。
    ///
    /// `superseded` 与 `rejected` 必须区分：
    /// - `rejected` 是「这条知识是错的」
    /// - `superseded` 是「这条知识曾经对，现在被更新的知识取代了」
    ///
    /// 这个区分正是检索层能同时回答「现任 CEO 是谁」和「前任 CEO 是谁」的前提。
    pub enum ClaimStatus {
        Draft => "draft",
        Candidate => "candidate",
        Verified => "verified",
        Rejected => "rejected",
        Archived => "archived",
        Superseded => "superseded",
    }
}

impl ClaimStatus {
    pub const DEFAULT: ClaimStatus = ClaimStatus::Candidate;

    /// 是否可作为「当前知识」的候选。
    ///
    /// 注意 `superseded` 在这里**仍然返回 true**：它进入候选集后由
    /// [`crate::domain::evolution::temporal::resolve_current`] 决定去留——
    /// 若没有任何当前知识覆盖同一个 (subject, predicate)，它会被保留并标记
    /// 为历史，以便回答关于过去的问题。若在此处直接排除，历史就答不出来了。
    pub fn participates_in_retrieval(&self) -> bool {
        matches!(
            self,
            ClaimStatus::Candidate | ClaimStatus::Verified | ClaimStatus::Superseded
        )
    }

    /// 是否参与「当前知识」的计算。
    pub fn counts_as_current(&self) -> bool {
        matches!(self, ClaimStatus::Verified | ClaimStatus::Candidate)
    }
}

string_enum! {
    /// Claim 的语义类型（8 个）。
    pub enum ClaimType {
        Factual => "factual",
        Definitional => "definitional",
        Causal => "causal",
        Comparative => "comparative",
        Evaluative => "evaluative",
        Predictive => "predictive",
        Normative => "normative",
        Hypothetical => "hypothetical",
    }
}

impl ClaimType {
    pub const DEFAULT: ClaimType = ClaimType::Factual;

    /// 归一化并接受参考实现的别名表。
    pub fn canonical(raw: &str) -> AppResult<ClaimType> {
        let key = raw.trim().to_ascii_lowercase();
        let mapped = match key.as_str() {
            "fact" => "factual",
            "definition" => "definitional",
            "cause" | "causation" | "causality" => "causal",
            "comparison" => "comparative",
            "evaluation" => "evaluative",
            "prediction" => "predictive",
            "norm" => "normative",
            "hypothesis" => "hypothetical",
            other => other,
        };
        mapped
            .parse::<ClaimType>()
            .map_err(|_| AppError::Domain(format!("未注册的 Claim Type: {raw:?}")))
    }
}

string_enum! {
    /// 极性（2 个）。
    pub enum Polarity {
        Positive => "positive",
        Negative => "negative",
    }
}

impl Polarity {
    pub const DEFAULT: Polarity = Polarity::Positive;

    pub fn canonical(raw: &str) -> AppResult<Polarity> {
        let key = raw.trim().to_ascii_lowercase();
        let mapped = match key.as_str() {
            "affirmative" | "true" | "yes" => "positive",
            "negated" | "false" | "no" => "negative",
            other => other,
        };
        mapped
            .parse::<Polarity>()
            .map_err(|_| AppError::Domain(format!("未注册的极性: {raw:?}")))
    }

    pub fn is_positive(&self) -> bool {
        matches!(self, Polarity::Positive)
    }
}

string_enum! {
    /// 语气（6 个）。
    ///
    /// 注意：**条件**不在这里。参考实现把条件放在 `context` 里，
    /// wiki-ya 用独立的 `condition` 字段承载（决策 D3），
    /// 因为「是否带条件」是归一化判定的一项要求（`no_material_unresolved_condition`），
    /// 混在语气枚举里会丢掉这个信号。
    pub enum Modality {
        Asserted => "asserted",
        Possible => "possible",
        Probable => "probable",
        Capable => "capable",
        Necessary => "necessary",
        Recommended => "recommended",
    }
}

impl Modality {
    pub const DEFAULT: Modality = Modality::Asserted;

    pub fn canonical(raw: &str) -> AppResult<Modality> {
        let key = raw.trim().to_ascii_lowercase();
        let mapped = match key.as_str() {
            "actual" | "definite" | "certain" => "asserted",
            "maybe" => "possible",
            "likely" | "probably" => "probable",
            "can" | "able" => "capable",
            "must" | "required" => "necessary",
            "should" => "recommended",
            other => other,
        };
        mapped
            .parse::<Modality>()
            .map_err(|_| AppError::Domain(format!("未注册的语气: {raw:?}")))
    }

    /// 只有断言语气才允许升级为图谱里的 Relation。
    pub fn is_asserted(&self) -> bool {
        matches!(self, Modality::Asserted)
    }
}

/// Claim 的宾语。
///
/// 存储上落为 `object_id`（可空）与 `object_text`（可空）两列；
/// 这个枚举是二者的领域表达。
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimObject {
    Entity(EntityId),
    Literal(String),
    Number(f64),
    Boolean(bool),
    Date(Timestamp),
}

impl ClaimObject {
    /// 同一性键（实现缺口 G7）。
    ///
    /// 判定 `duplicate` / `coexists` 时必须比较归一化后的宾语，
    /// 而不是表面字符串：`Apple Inc.` 与 `Apple` 在实体消解之后应当相等。
    /// 因此有实体时用 `entity:{id}`，否则退化为大小写不敏感的文本。
    pub fn identity_key(&self) -> String {
        match self {
            ClaimObject::Entity(id) => format!("entity:{}", id.as_str()),
            ClaimObject::Literal(text) => format!("text:{}", text.trim().to_lowercase()),
            ClaimObject::Number(value) => format!("number:{value}"),
            ClaimObject::Boolean(value) => format!("bool:{value}"),
            ClaimObject::Date(value) => format!("date:{value}"),
        }
    }

    /// 是否已消解为实体（归一化的 `object_resolves_to_entity` 要求）。
    pub fn resolves_to_entity(&self) -> bool {
        matches!(self, ClaimObject::Entity(_))
    }
}

/// Claim。
#[derive(Debug, Clone)]
pub struct Claim {
    pub id: ClaimId,
    pub subject_id: EntityId,
    pub predicate: ClaimPredicate,
    pub object: Option<ClaimObject>,

    /// 自然语言陈述（可读句子），供 UI 与检索展示。
    pub content: Option<String>,
    /// 开放的上下文（JSON），承载那些不值得进枚举的细节。
    pub context: serde_json::Value,

    pub claim_type: ClaimType,
    pub polarity: Polarity,
    pub modality: Modality,
    /// 前提条件（决策 D3）。有实质条件时不允许升级为 Relation。
    pub condition: Option<String>,

    pub confidence: Option<f32>,
    pub status: ClaimStatus,

    /// 时间有效性（PRD §20）。来源待定，见决策 D2：
    /// 抽取不出时间时为 `None`，且 `None` 一律视为「始终有效」，
    /// 避免因为缺时间而丢掉知识。
    pub valid_from: Option<Timestamp>,
    pub valid_until: Option<Timestamp>,
    pub recorded_at: Timestamp,
    pub created_at: Timestamp,
}

impl Claim {
    /// 校验 confidence 落在 `[0,1]`（INV-18）。
    pub fn validate_confidence(confidence: Option<f32>) -> AppResult<Option<f32>> {
        match confidence {
            None => Ok(None),
            Some(value) if value.is_finite() && (0.0..=1.0).contains(&value) => Ok(Some(value)),
            Some(value) => Err(AppError::Invalid(format!(
                "confidence 必须落在 [0,1]，实际为 {value}"
            ))),
        }
    }

    /// 主语 + 谓语 + 宾语同一性，用于演化比较的确定性前置条件。
    pub fn identity_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.subject_id.as_str(),
            self.predicate.as_str(),
            self.object
                .as_ref()
                .map(ClaimObject::identity_key)
                .unwrap_or_default()
        )
    }

    /// 是否处于「当前」状态（不含 superseded）。
    pub fn is_current(&self) -> bool {
        !matches!(self.status, ClaimStatus::Superseded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_match_the_reference_registry() {
        assert_eq!(ClaimStatus::ALL.len(), 6);
        assert_eq!(ClaimType::ALL.len(), 8);
        assert_eq!(Polarity::ALL.len(), 2);
        assert_eq!(Modality::ALL.len(), 6);
    }

    #[test]
    fn aliases_normalise_to_canonical_values() {
        assert_eq!(ClaimType::canonical("Fact").unwrap(), ClaimType::Factual);
        assert_eq!(
            ClaimType::canonical("Causality").unwrap(),
            ClaimType::Causal
        );
        assert_eq!(Polarity::canonical("negated").unwrap(), Polarity::Negative);
        assert_eq!(Polarity::canonical("TRUE").unwrap(), Polarity::Positive);
        assert_eq!(Modality::canonical("must").unwrap(), Modality::Necessary);
        assert_eq!(Modality::canonical("likely").unwrap(), Modality::Probable);
    }

    #[test]
    fn unknown_aliases_are_rejected() {
        assert!(ClaimType::canonical("vibes").is_err());
        assert!(Polarity::canonical("sideways").is_err());
        assert!(Modality::canonical("maybe-not").is_err());
    }

    #[test]
    fn object_identity_prefers_resolved_entities() {
        let entity_id = EntityId::from_raw("e1");
        let a = ClaimObject::Entity(entity_id.clone());
        let b = ClaimObject::Entity(entity_id);
        assert_eq!(a.identity_key(), b.identity_key());

        let text_a = ClaimObject::Literal("  Apple Inc. ".into());
        let text_b = ClaimObject::Literal("apple inc.".into());
        assert_eq!(text_a.identity_key(), text_b.identity_key());
        assert_ne!(a.identity_key(), text_a.identity_key());
    }

    #[test]
    fn only_asserted_modality_is_graph_ready() {
        assert!(Modality::Asserted.is_asserted());
        assert!(!Modality::Possible.is_asserted());
        assert!(!Modality::Recommended.is_asserted());
    }

    #[test]
    fn confidence_is_range_checked() {
        assert_eq!(Claim::validate_confidence(Some(0.5)).unwrap(), Some(0.5));
        assert_eq!(Claim::validate_confidence(None).unwrap(), None);
        assert!(Claim::validate_confidence(Some(1.5)).is_err());
        assert!(Claim::validate_confidence(Some(f32::NAN)).is_err());
    }

    #[test]
    fn superseded_claims_still_take_part_in_retrieval() {
        assert!(ClaimStatus::Superseded.participates_in_retrieval());
        assert!(!ClaimStatus::Superseded.counts_as_current());
        assert!(!ClaimStatus::Rejected.participates_in_retrieval());
    }
}
