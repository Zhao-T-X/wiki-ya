//! 受控注册表 —— wiki-ya 的版本化事实来源。
//!
//! 设计要点：
//!
//! 1. **原样迁移**（TDD §71）：JSON 从参考实现逐字节搬过来，未做任何改写。
//!    改一个字母就会让存量知识在归一化时落到不同取值上。
//! 2. **编译期内嵌**（`include_str!`）：打包后的应用不依赖运行期文件路径，
//!    也不会出现"用户不小心删了 schemas 目录导致程序行为改变"。
//! 3. **启动自检**（[`self_check`]）：断言 JSON 与 Rust 枚举逐项一致，
//!    防的是"往 JSON 加了一个谓词但忘了加枚举分支"这类静默漂移。
//! 4. **内容指纹**（[`version`]）：任何注册表改动都会改变指纹，
//!    进而改变所有 Context Cache Key —— 失效由构造保证，无需手工清理。

use std::sync::OnceLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::domain::ontology::entity_type::EntityType;
use crate::domain::ontology::normalization::NormalizationOutcome;
use crate::domain::ontology::predicate::{ClaimPredicate, RelationPredicate};
use crate::error::{AppError, AppResult};

const ENTITY_TYPE_REGISTRY: &str = include_str!("../../../registries/entity-type-registry.json");
const CLAIM_PREDICATE_REGISTRY: &str =
    include_str!("../../../registries/claim-predicate-registry.json");
const RELATION_PREDICATE_REGISTRY: &str =
    include_str!("../../../registries/relation-predicate-registry.json");
const NORMALIZATION_RULES: &str =
    include_str!("../../../registries/relation-normalization-rules.json");

/// 参与指纹计算的全部注册表（按文件名排序，保证指纹稳定）。
const REGISTRY_FILES: &[(&str, &str)] = &[
    ("claim-predicate-registry.json", CLAIM_PREDICATE_REGISTRY),
    ("entity-type-registry.json", ENTITY_TYPE_REGISTRY),
    ("relation-normalization-rules.json", NORMALIZATION_RULES),
    ("relation-predicate-registry.json", RELATION_PREDICATE_REGISTRY),
];

/// 归一化规则里出现、但**未注册**为受控谓词的遗留词。
///
/// 参考实现带着这个不一致：`similar_to` / `different_from` 出现在
/// `claim_only_predicates` 与 `always_claim_only_predicates` 中，
/// 却不在 `claim-predicate-registry.json` 里。
///
/// 处理方式：**不为它们补充注册表**（一旦注册，抽取就会开始产出它们，
/// 而存量数据里根本没有——那才是真正的破坏），而是显式白名单化，
/// 让自检仍然能抓住**未来**新增的漂移。
const LEGACY_UNREGISTERED_RULE_WORDS: &[&str] = &["similar_to", "different_from"];

#[derive(Debug, Clone, Deserialize)]
struct EntityTypeRegistryFile {
    #[allow(dead_code)]
    version: String,
    entity_types: Vec<EntityTypeEntry>,
}

/// 一条 Entity Type 注册项。
#[derive(Debug, Clone, Deserialize)]
pub struct EntityTypeEntry {
    #[serde(rename = "type")]
    pub type_name: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct ClaimPredicateRegistryFile {
    #[allow(dead_code)]
    version: String,
    claim_predicates: Vec<String>,
}

/// 一条 Relation 谓语规格。
#[derive(Debug, Clone, Deserialize)]
pub struct RelationPredicateSpec {
    pub predicate: String,
    pub inverse_label: String,
    pub symmetric: bool,
    pub transitive: bool,
    pub source_types: Vec<String>,
    pub target_types: Vec<String>,
    pub requires_explicit_evidence: bool,
}

#[derive(Debug, Deserialize)]
struct RelationPredicateRegistryFile {
    #[allow(dead_code)]
    version: String,
    relation_predicates: Vec<RelationPredicateSpec>,
}

/// Relation ↔ Claim 的降级/升级规则。
#[derive(Debug, Clone, Deserialize)]
pub struct NormalizationRules {
    pub outcomes: Vec<String>,
    pub direct_relation_requirements: Vec<String>,
    pub claim_only_predicates: Vec<String>,
    pub never_semantically_upgrade: Vec<Vec<String>>,
    pub context_sensitive_predicates: Vec<String>,
    pub always_claim_only_predicates: Vec<String>,
}

/// 加载完成的注册表。
#[derive(Debug)]
pub struct Registry {
    pub entity_types: Vec<EntityTypeEntry>,
    pub claim_predicates: Vec<ClaimPredicate>,
    pub relation_predicates: Vec<RelationPredicate>,
    pub relation_specs: Vec<RelationPredicateSpec>,
    pub normalization: NormalizationRules,
    /// 内容指纹（sha256 前 12 位十六进制），随任何注册表改动而变化。
    pub fingerprint: String,
}

impl Registry {
    /// 查一个 Relation 谓语的规格。
    pub fn relation_spec(&self, predicate: RelationPredicate) -> Option<&RelationPredicateSpec> {
        let target = predicate.as_str();
        self.relation_specs
            .iter()
            .find(|spec| spec.predicate == target)
    }

    /// 查一个 Entity Type 的描述（前端词表浏览器用）。
    pub fn entity_type_description(&self, entity_type: EntityType) -> Option<&str> {
        let target = entity_type.as_str();
        self.entity_types
            .iter()
            .find(|entry| entry.type_name == target)
            .map(|entry| entry.description.as_str())
    }

    /// 该谓语是否被规则强制为"只能是 Claim"。
    pub fn is_always_claim_only(&self, predicate: &str) -> bool {
        self.normalization
            .always_claim_only_predicates
            .iter()
            .any(|p| p == predicate)
    }

    /// 该谓语是否属于"需要显式证据、上下文敏感"的那一类。
    pub fn is_context_sensitive(&self, predicate: &str) -> bool {
        self.normalization
            .context_sensitive_predicates
            .iter()
            .any(|p| p == predicate)
    }

    /// 是否存在从 `from` 到 `to` 的语义升级被禁止（如 `uses` → `depends_on`）。
    pub fn is_forbidden_upgrade(&self, from: &str, to: &str) -> bool {
        self.normalization
            .never_semantically_upgrade
            .iter()
            .any(|pair| pair.len() == 2 && pair[0] == from && pair[1] == to)
    }
}

/// 全局注册表（进程内只加载一次）。
pub fn registry() -> &'static Registry {
    static CELL: OnceLock<Registry> = OnceLock::new();
    CELL.get_or_init(|| {
        build().unwrap_or_else(|err| {
            // 注册表是静态不变量：加载失败意味着程序无法正确理解任何知识，
            // 此时继续运行只会产生错误的知识，因此宁可立即崩溃。
            panic!("注册表加载失败，wiki-ya 无法保证知识正确性：{err}");
        })
    })
}

/// 注册表内容指纹。
pub fn version() -> &'static str {
    &registry().fingerprint
}

fn build() -> AppResult<Registry> {
    let entity_file: EntityTypeRegistryFile = serde_json::from_str(ENTITY_TYPE_REGISTRY)
        .map_err(|e| AppError::Internal(format!("entity-type-registry.json 解析失败：{e}")))?;
    let claim_file: ClaimPredicateRegistryFile = serde_json::from_str(CLAIM_PREDICATE_REGISTRY)
        .map_err(|e| AppError::Internal(format!("claim-predicate-registry.json 解析失败：{e}")))?;
    let relation_file: RelationPredicateRegistryFile =
        serde_json::from_str(RELATION_PREDICATE_REGISTRY)
            .map_err(|e| AppError::Internal(format!("relation-predicate-registry.json 解析失败：{e}")))?;
    let normalization: NormalizationRules = serde_json::from_str(NORMALIZATION_RULES)
        .map_err(|e| AppError::Internal(format!("relation-normalization-rules.json 解析失败：{e}")))?;

    // 谓词字符串在加载时就转成受控枚举：JSON 里出现未注册词会立刻失败，
    // 而不是等到某次抽取才崩。
    let claim_predicates = claim_file
        .claim_predicates
        .iter()
        .map(|p| {
            p.parse::<ClaimPredicate>()
                .map_err(|e| AppError::Internal(format!("claim 注册表含非法谓语 {p:?}：{e}")))
        })
        .collect::<AppResult<Vec<_>>>()?;

    let relation_predicates = relation_file
        .relation_predicates
        .iter()
        .map(|spec| {
            spec.predicate
                .parse::<RelationPredicate>()
                .map_err(|e| AppError::Internal(format!("relation 注册表含非法谓语 {:?}：{e}", spec.predicate)))
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(Registry {
        entity_types: entity_file.entity_types,
        claim_predicates,
        relation_predicates,
        relation_specs: relation_file.relation_predicates,
        normalization,
        fingerprint: compute_fingerprint(),
    })
}

/// 对全部注册表文件名 + 内容做 SHA-256，取前 12 位。
///
/// 相比参考实现的 SHA-1 仅做算法升级：指纹只在本应用内部用作
/// Cache Key 组成，不跨系统比对，因此不存在兼容问题。
fn compute_fingerprint() -> String {
    let mut hasher = Sha256::new();
    for (name, content) in REGISTRY_FILES {
        hasher.update(name.as_bytes());
        hasher.update(content.as_bytes());
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..12].to_string()
}

/// 启动自检：JSON 与 Rust 枚举必须逐项一致（INV-12/INV-13）。
///
/// 这些检查放在运行期而不是只在测试里，是因为它们守护的是"抽取结果的合法性"，
/// 一旦失配，产出的知识会带着非法谓语落库，事后无法自动修复。
pub fn self_check() -> AppResult<()> {
    let reg = registry();

    if reg.entity_types.len() != EntityType::ALL.len() {
        return Err(AppError::Internal(format!(
            "Entity Type 数量不一致：JSON {} 项，代码 {} 项",
            reg.entity_types.len(),
            EntityType::ALL.len()
        )));
    }
    for (index, entry) in reg.entity_types.iter().enumerate() {
        let expected = EntityType::ALL[index];
        if entry.type_name != expected.as_str() {
            return Err(AppError::Internal(format!(
                "Entity Type 第 {index} 项不一致：JSON {:?}，代码 {:?}",
                entry.type_name,
                expected.as_str()
            )));
        }
    }

    if reg.claim_predicates != ClaimPredicate::ALL {
        return Err(AppError::Internal(
            "Claim 谓语注册表与代码枚举不一致（顺序敏感：注册表顺序即优先级）".into(),
        ));
    }

    if reg.relation_predicates != RelationPredicate::ALL {
        return Err(AppError::Internal(
            "Relation 谓语注册表与代码枚举不一致（顺序敏感）".into(),
        ));
    }

    for spec in &reg.relation_specs {
        if spec.inverse_label.trim().is_empty() {
            return Err(AppError::Internal(format!(
                "{} 缺少 inverse_label：图谱双向展示依赖它",
                spec.predicate
            )));
        }
        if spec.source_types.is_empty() || spec.target_types.is_empty() {
            return Err(AppError::Internal(format!(
                "{} 的 source_types/target_types 不能为空（空集合会使关系永远无法通过校验）",
                spec.predicate
            )));
        }
    }

    let declared = &reg.normalization.outcomes;
    if declared.len() != NormalizationOutcome::ALL.len()
        || !declared
            .iter()
            .zip(NormalizationOutcome::ALL)
            .all(|(raw, expected)| raw == expected.as_str())
    {
        return Err(AppError::Internal(
            "归一化 outcome 注册表与代码枚举不一致".into(),
        ));
    }

    // 硬红线必须自洽：所有"永远是 Claim"的谓语都应同时出现在 claim_only 集合里。
    for predicate in &reg.normalization.always_claim_only_predicates {
        if !reg.normalization.claim_only_predicates.contains(predicate) {
            return Err(AppError::Internal(format!(
                "{predicate} 在 always_claim_only 中，却不在 claim_only 中：规则集自相矛盾"
            )));
        }
    }

    for predicate in reg
        .normalization
        .claim_only_predicates
        .iter()
        .chain(reg.normalization.always_claim_only_predicates.iter())
        .chain(reg.normalization.context_sensitive_predicates.iter())
    {
        check_rule_word(predicate)?;
    }

    for pair in &reg.normalization.never_semantically_upgrade {
        if pair.len() != 2 {
            return Err(AppError::Internal(format!(
                "never_semantically_upgrade 的每项必须是 [from, to] 两元素，实际：{pair:?}"
            )));
        }
        check_rule_word(&pair[0])?;
        check_rule_word(&pair[1])?;
    }

    Ok(())
}

/// 规则里出现的谓语必须是"已注册"或"已知遗留"二者之一。
fn check_rule_word(word: &str) -> AppResult<()> {
    if LEGACY_UNREGISTERED_RULE_WORDS.contains(&word) {
        return Ok(());
    }
    let registered = word
        .parse::<ClaimPredicate>()
        .is_ok()
        || word.parse::<RelationPredicate>().is_ok();
    if registered {
        Ok(())
    } else {
        Err(AppError::Internal(format!(
            "归一化规则引用了未注册的谓语 {word:?}：要么注册它，要么显式加入 LEGACY_UNREGISTERED_RULE_WORDS"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_check_passes() {
        self_check().expect("内嵌注册表必须与代码枚举一致");
    }

    #[test]
    fn fingerprint_is_stable_and_twelve_hex_chars() {
        let v = version();
        assert_eq!(v.len(), 12);
        assert!(v.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(v, version());
    }

    #[test]
    fn specs_are_reachable_by_enum() {
        let reg = registry();
        let spec = reg.relation_spec(RelationPredicate::TrainedOn).unwrap();
        assert_eq!(spec.source_types, vec!["Model".to_string()]);
        assert_eq!(spec.target_types, vec!["Dataset".to_string()]);
        assert!(spec.requires_explicit_evidence);
    }

    #[test]
    fn rule_helpers_reflect_the_registry() {
        let reg = registry();
        assert!(reg.is_always_claim_only("better_than"));
        assert!(!reg.is_always_claim_only("uses"));
        assert!(reg.is_context_sensitive("uses"));
        assert!(reg.is_forbidden_upgrade("uses", "depends_on"));
        assert!(!reg.is_forbidden_upgrade("depends_on", "uses"));
    }

    #[test]
    fn entity_type_descriptions_are_available_for_the_ui() {
        let reg = registry();
        assert!(reg
            .entity_type_description(EntityType::Software)
            .unwrap()
            .contains("software"));
    }
}
