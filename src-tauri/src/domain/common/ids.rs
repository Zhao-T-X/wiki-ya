//! 标识类型。
//!
//! 每个实体一个 newtype，而不是统一用 `String`：`claim_id` 和 `entity_id`
//! 混用在参考实现里是一个真实存在的 bug 来源，类型系统可以免费挡掉。

/// 生成一个字符串标识 newtype（UUID v4，文本存储）。
///
/// 之所以不用整数自增：知识条目会被导出/导入（PRD §43），
/// UUID 让跨库合并与迁移不需要重编号。
macro_rules! string_id {
    (
        $(#[$meta:meta])*
        $name:ident
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// 生成一个新标识。
            pub fn new() -> Self {
                Self(uuid::Uuid::new_v4().to_string())
            }

            /// 从已有值构造（迁移与测试用）。
            pub fn from_raw(raw: impl Into<String>) -> Self {
                Self(raw.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::error::AppError;

            fn from_str(raw: &str) -> ::std::result::Result<Self, Self::Err> {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err($crate::error::AppError::Invalid(format!(
                        "{} 不能为空",
                        stringify!($name)
                    )));
                }
                Ok(Self(trimmed.to_string()))
            }
        }

        impl ::std::convert::AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl ::rusqlite::types::ToSql for $name {
            fn to_sql(
                &self,
            ) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput<'_>> {
                Ok(::rusqlite::types::ToSqlOutput::from(self.0.as_str()))
            }
        }

        impl ::rusqlite::types::FromSql for $name {
            fn column_result(
                value: ::rusqlite::types::ValueRef<'_>,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                value.as_str().map(|s| Self(s.to_string()))
            }
        }
    };
}

string_id!(
    /// 原始文档。
    DocumentId
);
string_id!(
    /// 文档内的计算单位。
    ChunkId
);
string_id!(
    /// 稳定对象（Rust、Tauri、SQLite…）。
    EntityId
);
string_id!(
    /// 知识的核心单位。
    ClaimId
);
string_id!(
    /// 实体到实体的关系。
    RelationId
);
string_id!(
    /// Claim 之间的演化关系。
    ClaimRelationId
);
string_id!(
    /// Claim 与来源之间的桥。
    EvidenceId
);
string_id!(
    /// 事件。
    EventId
);
string_id!(
    /// 想法。
    IdeaId
);
string_id!(
    /// 未解决问题。
    QuestionId
);
string_id!(
    /// 研究任务。
    ResearchTaskId
);
string_id!(
    /// 待审核提案。
    ReviewId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_ids_are_rejected() {
        assert!("".parse::<EntityId>().is_err());
        assert!("   ".parse::<ClaimId>().is_err());
    }

    #[test]
    fn ids_are_opaque_but_roundtrip() {
        let id = EntityId::new();
        let parsed: EntityId = id.as_str().parse().unwrap();
        assert_eq!(id, parsed);
        assert_eq!(id.to_string(), id.as_str());
    }
}
