//! Relation —— 实体到实体的稳定关系。
//!
//! 与 Claim 的区别不是"存哪儿"，而是**语义强度**：
//! Relation 断言的是一个长期成立、值得进图谱的结构性事实
//! （`wiki-ya uses SQLite`），Claim 断言的是"某段文本说了什么"
//! （`wiki-ya supports async fn in trait`）。
//!
//! 因此 Relation 有 `requires_explicit_evidence` 的要求，
//! 并且**不存在 `superseded` 状态**：关系不会被更新取代，
//! 只有"当前成立 / 已废止"（决策 D6）。

use crate::domain::common::ids::{EntityId, RelationId};
use crate::domain::ontology::entity_type::EntityType;
use crate::domain::ontology::predicate::{normalize_predicate, RelationPredicate};
use crate::domain::ontology::registry::registry;
use crate::error::{AppError, AppResult};
use crate::string_enum;

string_enum! {
    /// 关系状态。
    ///
    /// 与 [`crate::domain::ontology::entity::EntityStatus`] 同为 5 值，
    /// 刻意**不含** `superseded`（决策 D6）。
    pub enum RelationStatus {
        Draft => "draft",
        Candidate => "candidate",
        Verified => "verified",
        Rejected => "rejected",
        Archived => "archived",
    }
}

impl RelationStatus {
    pub const DEFAULT: RelationStatus = RelationStatus::Candidate;

    pub fn is_live(&self) -> bool {
        !matches!(self, RelationStatus::Rejected | RelationStatus::Archived)
    }
}

/// 实体关系。
#[derive(Debug, Clone)]
pub struct Relation {
    pub id: RelationId,
    pub source_id: EntityId,
    pub predicate: RelationPredicate,
    pub target_id: EntityId,
    pub confidence: Option<f32>,
    pub status: RelationStatus,
}

impl Relation {
    /// 构造关系，并在此处强制两条不变量（INV-13 / INV-14）。
    ///
    /// 传入的是自由文本谓语，本函数负责归一与越界拒绝——
    /// 这样调用方就无法"绕过注册表"直接构造一个非法关系。
    pub fn new(
        source_id: EntityId,
        predicate: &str,
        target_id: EntityId,
        source_types: &[EntityType],
        target_types: &[EntityType],
    ) -> AppResult<Relation> {
        if source_id == target_id {
            return Err(AppError::Domain(
                "关系两端不能是同一个实体（自环会让图谱查询失去意义）".into(),
            ));
        }

        let predicate = RelationPredicate::canonical(predicate)?;

        let spec = registry()
            .relation_spec(predicate)
            .ok_or_else(|| AppError::Domain(format!("{predicate} 缺少注册表规格")))?;

        // INV-14：两端类型都必须落在注册表声明的 source_types / target_types 内
        if !endpoint_allowed(source_types, &spec.source_types) {
            return Err(AppError::Domain(format!(
                "{} 的源端类型 {:?} 不在允许集合 {:?} 内",
                predicate,
                type_names(source_types),
                spec.source_types
            )));
        }
        if !endpoint_allowed(target_types, &spec.target_types) {
            return Err(AppError::Domain(format!(
                "{} 的目标端类型 {:?} 不在允许集合 {:?} 内",
                predicate,
                type_names(target_types),
                spec.target_types
            )));
        }

        Ok(Relation {
            id: RelationId::new(),
            source_id,
            predicate,
            target_id,
            confidence: None,
            status: RelationStatus::DEFAULT,
        })
    }

    /// 反向关系的可读谓语（`uses` → `used_by`）。
    ///
    /// 用于图谱的双向展示：同一条边从两端看是不同的说法，
    /// 但**只存一行**（不写反向冗余行，避免两份真相）。
    pub fn inverse_label(&self) -> Option<&str> {
        registry()
            .relation_spec(self.predicate)
            .map(|spec| spec.inverse_label.as_str())
    }
}

/// 注册表类型约束校验（`*` 通配，见 INV-14）。
///
/// 空类型集合视为**不通过**：Entity 在写入时保证至少有一个类型，
/// 因此空集只可能来自调用方漏填，那属于 bug，不该被放过。
fn endpoint_allowed(actual: &[EntityType], allowed: &[String]) -> bool {
    !actual.is_empty() && actual.iter().all(|t| t.matches_spec(allowed))
}

fn type_names(types: &[EntityType]) -> Vec<&'static str> {
    types.iter().map(|t| t.as_str()).collect()
}

/// 规范化一个自由文本 predicate，供"这一列要建索引"的调用方使用。
pub fn normalize_for_storage(raw: &str) -> String {
    normalize_predicate(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_self_loops() {
        let id = EntityId::new();
        let err = Relation::new(
            id.clone(),
            "uses",
            id,
            &[EntityType::Software],
            &[EntityType::Software],
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn rejects_unregistered_predicates() {
        let err = Relation::new(
            EntityId::new(),
            "vibes_with",
            EntityId::new(),
            &[EntityType::Software],
            &[EntityType::Software],
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn enforces_declared_endpoint_types() {
        // develops: Person|Organization → Product|Software|...
        let err = Relation::new(
            EntityId::new(),
            "develops",
            EntityId::new(),
            &[EntityType::Dataset],
            &[EntityType::Software],
        )
        .unwrap_err();
        assert!(err.to_string().contains("源端类型"));
    }

    #[test]
    fn accepts_a_registered_combination() {
        let r = Relation::new(
            EntityId::new(),
            "Develops",
            EntityId::new(),
            &[EntityType::Organization],
            &[EntityType::Software],
        )
        .unwrap();
        assert_eq!(r.predicate, RelationPredicate::Develops);
        assert_eq!(r.inverse_label(), Some("developed_by"));
    }

    #[test]
    fn relation_status_has_no_superseded() {
        assert!(!RelationStatus::ALL.iter().any(|s| s.as_str() == "superseded"));
    }
}
