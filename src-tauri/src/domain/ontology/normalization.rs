//! 归一化 —— 决定一段抽取结果到底是 Relation、Claim，还是垃圾。
//!
//! 这是 PRD §13「Relation 与 Claim 不混淆」的**实现手段**。
//! 之所以要有它：`uses` 同时出现在两套注册表里，光看谓语无法判断
//! 该写 `relations` 还是 `claims`。判定必须由规则给出，不能靠调用方心情。
//!
//! 四种结果：
//!
//! - `DIRECT_RELATION`      十项要求全满足，可写 `relations`（长期图谱价值）
//! - `CONDITIONAL_RELATION` 是注册关系谓语，但条件不齐 → 降级为 Claim
//! - `CLAIM_ONLY`           规则明令只能作 Claim（如 `supports`、`better_than`）
//! - `REJECTED`             谓语压根没注册 → 不写任何东西
//!
//! 全部判定都是确定性的、可解释的（返回未满足项清单），不含 LLM。

use crate::domain::ontology::entity_type::EntityType;
use crate::domain::ontology::predicate::normalize_predicate;
use crate::domain::ontology::registry::registry;
use crate::string_enum;

string_enum! {
    /// 归一化判定结果。取值与 `relation-normalization-rules.json` 的 `outcomes` 一致。
    pub enum NormalizationOutcome {
        DirectRelation => "DIRECT_RELATION",
        ConditionalRelation => "CONDITIONAL_RELATION",
        ClaimOnly => "CLAIM_ONLY",
        Rejected => "REJECTED",
    }
}

impl NormalizationOutcome {
    /// 该结果是否允许写入 `relations` 表。
    pub fn allows_relation_row(&self) -> bool {
        matches!(self, NormalizationOutcome::DirectRelation)
    }

    /// 该结果是否需要人工介入。
    ///
    /// `ConditionalRelation` 会降级为 Claim 而不打扰用户；
    /// `Rejected` 由抽取层直接丢弃并记录，也不进审核队列。
    pub fn requires_review(&self) -> bool {
        false
    }
}

/// 待判定的候选（由抽取层或人工录入填充）。
///
/// 只收事实，不做判断——判断由 [`classify`] 完成。
#[derive(Debug, Clone, Default)]
pub struct RelationCandidate {
    pub predicate: String,

    /// 主语是否已消解到 entity。
    pub subject_resolves_to_entity: bool,
    /// 宾语是否已消解到 entity（决定它能否成为图的边）。
    pub object_resolves_to_entity: bool,
    /// 谓语的规范语义是否由文本显式支撑，而非推断。
    pub explicit_canonical_semantics: bool,
    /// 极性为正（`negative` 一律不能建关系）。
    pub positive_polarity: bool,
    /// 语气为断言（`possible` / `recommended` 等一律不能建关系）。
    pub asserted_modality: bool,
    /// 没有未解决的实质性前提条件。
    pub no_material_unresolved_condition: bool,
    /// 存在可直接引用的证据。
    pub valid_direct_evidence: bool,
    /// 具备长期图谱价值（不是一次性观察）。
    pub long_term_graph_value: bool,

    pub source_types: Vec<EntityType>,
    pub target_types: Vec<EntityType>,
}

/// 判定结果，带可解释的原因。
#[derive(Debug, Clone)]
pub struct NormalizationDecision {
    pub outcome: NormalizationOutcome,
    /// 归一化后的谓语（小写 + 下划线折叠）。
    pub normalized_predicate: String,
    /// 未满足的要求 / 触发降级的原因，按注册表顺序。
    pub reasons: Vec<String>,
}

impl NormalizationDecision {
    pub fn allows_relation_row(&self) -> bool {
        self.outcome.allows_relation_row()
    }
}

/// 执行判定。
pub fn classify(candidate: &RelationCandidate) -> NormalizationDecision {
    let reg = registry();
    let normalized = normalize_predicate(&candidate.predicate);

    if normalized.is_empty() {
        return NormalizationDecision {
            outcome: NormalizationOutcome::Rejected,
            normalized_predicate: normalized,
            reasons: vec!["谓语为空，无法归一化".into()],
        };
    }

    let relation_predicate = reg
        .relation_predicates
        .iter()
        .find(|p| p.as_str() == normalized);
    let claim_predicate = reg
        .claim_predicates
        .iter()
        .find(|p| p.as_str() == normalized);

    // 硬红线优先：即使谓词同时注册为关系，规则说"只能是 Claim"就必须从命。
    if reg.is_always_claim_only(&normalized) {
        let reasons = vec![format!(
            "`{normalized}` 属于 always_claim_only_predicates（语义不稳定，只能作为 Claim 存在）"
        )];
        return NormalizationDecision {
            outcome: NormalizationOutcome::ClaimOnly,
            normalized_predicate: normalized,
            reasons,
        };
    }

    let Some(_) = relation_predicate else {
        if claim_predicate.is_some() {
            let reasons = vec![format!("`{normalized}` 未注册为 Relation 谓语")];
            return NormalizationDecision {
                outcome: NormalizationOutcome::ClaimOnly,
                normalized_predicate: normalized,
                reasons,
            };
        }
        let reasons = vec![format!("`{normalized}` 不在任何受控注册表中")];
        return NormalizationDecision {
            outcome: NormalizationOutcome::Rejected,
            normalized_predicate: normalized,
            reasons,
        };
    };

    // 逐项检查 direct_relation_requirements，顺序即注册表顺序，便于前端逐条展示。
    let mut unmet = Vec::new();
    for requirement in &reg.normalization.direct_relation_requirements {
        match requirement.as_str() {
            "subject_resolves_to_entity" => {
                if !candidate.subject_resolves_to_entity {
                    unmet.push("主语未消解为实体".to_string());
                }
            }
            "object_resolves_to_entity" => {
                if !candidate.object_resolves_to_entity {
                    unmet.push("宾语未消解为实体".to_string());
                }
            }
            "predicate_in_relation_registry" => {}
            "explicit_canonical_semantics" => {
                if !candidate.explicit_canonical_semantics {
                    unmet.push("规范语义未被文本显式支撑".to_string());
                }
            }
            "positive_polarity" => {
                if !candidate.positive_polarity {
                    unmet.push("极性为负，不能建立正向关系".to_string());
                }
            }
            "asserted_modality" => {
                if !candidate.asserted_modality {
                    unmet.push("语气非断言（可能/建议等），不能建立关系".to_string());
                }
            }
            "no_material_unresolved_condition" => {
                if !candidate.no_material_unresolved_condition {
                    unmet.push("存在未解决的实质性前提条件".to_string());
                }
            }
            "valid_direct_evidence" => {
                if !candidate.valid_direct_evidence {
                    unmet.push("缺少可直接引用的证据".to_string());
                }
            }
            "long_term_graph_value" => {
                if !candidate.long_term_graph_value {
                    unmet.push("不具备长期图谱价值".to_string());
                }
            }
            "source_target_types_allowed" => {
                if let Some(spec) = reg.relation_specs.iter().find(|s| s.predicate == normalized) {
                    let source_ok = !candidate.source_types.is_empty()
                        && candidate
                            .source_types
                            .iter()
                            .all(|t| t.matches_spec(&spec.source_types));
                    let target_ok = !candidate.target_types.is_empty()
                        && candidate
                            .target_types
                            .iter()
                            .all(|t| t.matches_spec(&spec.target_types));
                    if !source_ok {
                        unmet.push(format!(
                            "源端类型 {:?} 不在允许集合 {:?} 内",
                            type_names(&candidate.source_types),
                            spec.source_types
                        ));
                    }
                    if !target_ok {
                        unmet.push(format!(
                            "目标端类型 {:?} 不在允许集合 {:?} 内",
                            type_names(&candidate.target_types),
                            spec.target_types
                        ));
                    }
                }
            }
            // 注册表出现未知要求说明 self_check 漏了这条，保守起见判为未满足。
            other => unmet.push(format!("未知要求 `{other}`（请补充判定逻辑）")),
        }
    }

    if unmet.is_empty() {
        NormalizationDecision {
            outcome: NormalizationOutcome::DirectRelation,
            normalized_predicate: normalized,
            reasons: vec!["直接关系要求全部满足".into()],
        }
    } else {
        NormalizationDecision {
            outcome: NormalizationOutcome::ConditionalRelation,
            normalized_predicate: normalized,
            reasons: unmet,
        }
    }
}

fn type_names(types: &[EntityType]) -> Vec<&'static str> {
    types.iter().map(|t| t.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fully_satisfied(predicate: &str) -> RelationCandidate {
        RelationCandidate {
            predicate: predicate.to_string(),
            subject_resolves_to_entity: true,
            object_resolves_to_entity: true,
            explicit_canonical_semantics: true,
            positive_polarity: true,
            asserted_modality: true,
            no_material_unresolved_condition: true,
            valid_direct_evidence: true,
            long_term_graph_value: true,
            source_types: vec![EntityType::Person],
            target_types: vec![EntityType::Software],
        }
    }

    #[test]
    fn fully_satisfied_relation_predicate_becomes_a_relation() {
        let decision = classify(&fully_satisfied("develops"));
        assert_eq!(decision.outcome, NormalizationOutcome::DirectRelation);
        assert!(decision.allows_relation_row());
    }

    #[test]
    fn missing_requirement_downgrades_to_claim_with_reasons() {
        let mut candidate = fully_satisfied("develops");
        candidate.object_resolves_to_entity = false;
        let decision = classify(&candidate);
        assert_eq!(
            decision.outcome,
            NormalizationOutcome::ConditionalRelation
        );
        assert!(!decision.allows_relation_row());
        assert!(decision.reasons.iter().any(|r| r.contains("宾语")));
    }

    #[test]
    fn always_claim_only_wins_even_when_registered_as_relation() {
        // uses 同时注册在两套表里，但规则把它划入上下文敏感；
        // 即便如此，always_claim_only 的硬红线必须优先于一切。
        let decision = classify(&fully_satisfied("better_than"));
        assert_eq!(decision.outcome, NormalizationOutcome::ClaimOnly);
    }

    #[test]
    fn negative_polarity_cannot_become_a_relation() {
        let mut candidate = fully_satisfied("uses");
        candidate.positive_polarity = false;
        let decision = classify(&candidate);
        assert_eq!(
            decision.outcome,
            NormalizationOutcome::ConditionalRelation
        );
        assert!(decision.reasons.iter().any(|r| r.contains("极性")));
    }

    #[test]
    fn endpoint_type_violation_is_reported() {
        let mut candidate = fully_satisfied("develops");
        candidate.source_types = vec![EntityType::Dataset];
        let decision = classify(&candidate);
        assert!(decision.reasons.iter().any(|r| r.contains("源端类型")));
    }

    #[test]
    fn unregistered_predicate_is_rejected_not_silently_stored() {
        let candidate = fully_satisfied("vibes_with");
        let decision = classify(&candidate);
        assert_eq!(decision.outcome, NormalizationOutcome::Rejected);
    }

    #[test]
    fn claim_predicate_that_is_not_a_relation_stays_a_claim() {
        let decision = classify(&fully_satisfied("tested_on"));
        assert_eq!(decision.outcome, NormalizationOutcome::ClaimOnly);
    }
}
