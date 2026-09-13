//! Entity —— 稳定对象。
//!
//! Entity 只表达"世界上的一个东西"，不表达关于它的任何主张（那是 Claim）。
//! 类型是受控的多值集合：`types[0]` 即 `primary_type`，
//! 因为同一个东西可以同时是 `Software` 与 `Product`。

use crate::domain::common::ids::EntityId;
use crate::domain::common::Timestamp;
use crate::domain::ontology::entity_type::EntityType;
use crate::string_enum;

string_enum! {
    /// 实体在工作流中的生命周期状态。
    ///
    /// 注意这里**没有** `superseded`：实体不会被"取代"，
    /// 被取代的是关于它的 Claim（决策 D6）。
    pub enum EntityStatus {
        Draft => "draft",
        Candidate => "candidate",
        Verified => "verified",
        Rejected => "rejected",
        Archived => "archived",
    }
}

impl EntityStatus {
    /// 默认状态：LLM 或人工新建的实体一律从 `candidate` 开始。
    pub const DEFAULT: EntityStatus = EntityStatus::Candidate;

    /// 是否参与检索与图谱展示。
    ///
    /// `rejected` 与 `archived` 都不参与——归档是"别再提它"，
    /// 与"当前知识"的判定无关（归档实体不影响 claim 的历史有效性）。
    pub fn is_live(&self) -> bool {
        !matches!(self, EntityStatus::Rejected | EntityStatus::Archived)
    }
}

/// 实体别名。
///
/// `normalized` 在写入时算好并建索引，因为消解的 Alias 级查询必须走索引
/// （INV-04：同一实体内别名唯一，不同实体可共享别名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityAlias {
    pub entity_id: EntityId,
    pub alias: String,
    pub normalized: String,
}

/// 稳定对象。
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub primary_type: EntityType,
    pub types: Vec<EntityType>,
    pub description: Option<String>,
    pub properties: serde_json::Value,
    pub status: EntityStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Entity {
    /// 新建实体：名称与类型先归一，再落库。
    ///
    /// `types` 为空时用 `EntityType::DEFAULT` 占位，且保证 `primary_type`
    /// 一定出现在 `types` 中——否则前端会出现"主类型不在类型列表里"的矛盾展示。
    pub fn new(name: &str, types: Vec<EntityType>) -> crate::error::AppResult<Entity> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return crate::error::AppResult::Err(crate::error::AppError::Invalid(
                "实体名称不能为空".into(),
            ));
        }

        let mut resolved = types;
        resolved.dedup();
        if resolved.is_empty() {
            resolved.push(EntityType::DEFAULT);
        }
        let primary = resolved[0];

        Ok(Entity {
            id: EntityId::new(),
            name: trimmed.to_string(),
            primary_type: primary,
            types: resolved,
            description: None,
            properties: serde_json::json!({}),
            status: EntityStatus::DEFAULT,
            created_at: String::new(),
            updated_at: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_entity_defaults_to_candidate_and_resource() {
        let e = Entity::new("Rust", vec![]).unwrap();
        assert_eq!(e.status, EntityStatus::Candidate);
        assert_eq!(e.primary_type, EntityType::Resource);
        assert_eq!(e.types.len(), 1);
    }

    #[test]
    fn primary_type_always_appears_in_types() {
        let e = Entity::new("SQLite", vec![EntityType::Software, EntityType::Product]).unwrap();
        assert_eq!(e.primary_type, EntityType::Software);
        assert!(e.types.contains(&EntityType::Software));
    }

    #[test]
    fn blank_names_are_rejected() {
        assert!(Entity::new("   ", vec![]).is_err());
    }

    #[test]
    fn archived_entities_leave_retrieval() {
        assert!(EntityStatus::Verified.is_live());
        assert!(!EntityStatus::Archived.is_live());
        assert!(!EntityStatus::Rejected.is_live());
    }
}
