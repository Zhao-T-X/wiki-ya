//! 受控 Entity Type（14 个，封闭集合）。
//!
//! 与 `registries/entity-type-registry.json` **同源**：`registry::self_check()`
//! 会在启动时断言两者逐字一致，因此不可能出现"代码支持 14 种、抽取 schema
//! 只认 5 种"这类漂移。
//!
//! PRD §10 明确禁止 LLM 自由创造类型——这条规则在这里由类型系统保证：
//! 越界字符串只能得到 `Err`，没有"其他"兜底分支。

use crate::error::{AppError, AppResult};
use crate::string_enum;

string_enum! {
    /// 实体类型。
    pub enum EntityType {
        Person => "Person",
        Organization => "Organization",
        Product => "Product",
        Software => "Software",
        Technology => "Technology",
        Method => "Method",
        Concept => "Concept",
        Theory => "Theory",
        Dataset => "Dataset",
        Model => "Model",
        Standard => "Standard",
        Protocol => "Protocol",
        Resource => "Resource",
        Location => "Location",
    }
}

impl EntityType {
    /// 无法归类时的落点。
    ///
    /// 注意：这只是**默认值**，不是"兜底"——调用方必须显式选择它，
    /// 不能把解析失败的输入偷偷变成 `Resource`。
    pub const DEFAULT: EntityType = EntityType::Resource;

    /// 把外部输入（抽取结果、旧库、用户输入）归一为受控类型。
    ///
    /// 接受：精确匹配 → 大小写不敏感匹配 → 参考实现的 legacy 映射。
    /// 其余一律报错。特别地 `project` 映射为 `Resource` 而**不是** `Product`，
    /// 这是参考实现的历史约定，迁移时必须保持，否则存量实体类型会变。
    pub fn canonical(raw: &str) -> AppResult<EntityType> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(AppError::Invalid("实体类型不能为空".into()));
        }

        // 1) 精确匹配（走受控 FromStr）
        if let Ok(exact) = trimmed.parse::<EntityType>() {
            return Ok(exact);
        }

        // 2) 大小写不敏感匹配
        for candidate in EntityType::ALL {
            if candidate.as_str().eq_ignore_ascii_case(trimmed) {
                return Ok(*candidate);
            }
        }

        // 3) legacy 映射
        let lower = trimmed.to_ascii_lowercase();
        let legacy = match lower.as_str() {
            "person" => Some(EntityType::Person),
            "organization" => Some(EntityType::Organization),
            "place" => Some(EntityType::Location),
            "concept" => Some(EntityType::Concept),
            "project" => Some(EntityType::Resource),
            _ => None,
        };
        if let Some(mapped) = legacy {
            return Ok(mapped);
        }

        Err(AppError::Domain(format!(
            "未注册的 Entity Type: {raw:?}（受控注册表共 {} 项，禁止新增）",
            EntityType::ALL.len()
        )))
    }

    /// 类型是否与注册表声明的类型集合相容（`*` 表示通配）。
    pub fn matches_spec(&self, allowed: &[String]) -> bool {
        allowed.iter().any(|t| t == "*" || t == self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_size_is_frozen_at_fourteen() {
        assert_eq!(EntityType::ALL.len(), 14);
    }

    #[test]
    fn legacy_project_maps_to_resource_not_product() {
        assert_eq!(EntityType::canonical("project").unwrap(), EntityType::Resource);
        assert_eq!(EntityType::canonical("PLACE").unwrap(), EntityType::Location);
        assert_eq!(EntityType::canonical("Software").unwrap(), EntityType::Software);
    }

    #[test]
    fn unknown_types_are_rejected_rather_than_defaulted() {
        let err = EntityType::canonical("Spacecraft").unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
    }

    #[test]
    fn wildcard_spec_accepts_everything() {
        assert!(EntityType::Model.matches_spec(&["*".to_string()]));
        assert!(!EntityType::Person.matches_spec(&["Dataset".to_string()]));
        assert!(EntityType::Dataset.matches_spec(&["Dataset".to_string()]));
    }
}
