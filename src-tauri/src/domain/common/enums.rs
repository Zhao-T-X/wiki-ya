//! 受控词表的实现底座。
//!
//! 本文件**只提供机制，不定义任何领域取值**——具体词表见
//! `domain/ontology`、`domain/knowledge`、`domain/evolution` 等模块。
//!
//! 设计要点（对应 `docs/领域枚举与不变量定义.md` INV-12）：
//!
//! 1. **封闭集合**：每个枚举的取值必须与 `docs/领域枚举与不变量定义.md` 的
//!    字面量逐字节一致，否则存量数据迁移后无法反序列化。
//! 2. **禁止静默兜底**：越界取值一律返回 `AppError::Domain`，绝不
//!    `unwrap_or_default()`。宁可报错，也不要把未知知识偷偷降级。
//! 3. **单一事实来源**：`as_str()` 同时供 Serde、SQLite `ToSql`、
//!    前端 JSON 使用，因此不可能出现三处字面量漂移。

/// 为字符串受控词表生成 `Serde` / `Display` / `FromStr` / `rusqlite` 双向往返实现。
///
/// ```ignore
/// string_enum! {
///     /// Claim 的生命周期状态。
///     pub enum ClaimStatus {
///         Draft => "draft",
///         Candidate => "candidate",
///         Verified => "verified",
///     }
/// }
/// ```
///
/// 生成内容：
/// - `as_str()` / `Display`：产出数据库与 JSON 中的规范字面量
/// - `FromStr` / `Deserialize`：仅接受规范字面量（别名归一由各模块另写 `canonical()`）
/// - `ToSql` / `FromSql`：可直接绑定到 `rusqlite` 语句与 `query_row` 映射
/// - `ALL`：全部取值的切片，供前端下拉与自检测试使用
#[macro_export]
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident => $lit:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        pub enum $name {
            $(
                $(#[$vmeta])*
                #[serde(rename = $lit)]
                $variant,
            )+
        }

        impl $name {
            /// 全部取值，顺序与注册表声明顺序一致（注册表顺序即优先级）。
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// 数据库列与 JSON 字段使用的规范字面量。
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $lit),+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::error::AppError;

            fn from_str(raw: &str) -> ::std::result::Result<Self, Self::Err> {
                match raw {
                    $($lit => Ok($name::$variant),)+
                    other => Err($crate::error::AppError::Domain(format!(
                        "{} 是受控词表，不接受取值 {:?}",
                        stringify!($name),
                        other
                    ))),
                }
            }
        }

        impl ::rusqlite::types::ToSql for $name {
            fn to_sql(
                &self,
            ) -> ::rusqlite::Result<::rusqlite::types::ToSqlOutput<'_>> {
                Ok(::rusqlite::types::ToSqlOutput::from(self.as_str()))
            }
        }

        impl ::rusqlite::types::FromSql for $name {
            fn column_result(
                value: ::rusqlite::types::ValueRef<'_>,
            ) -> ::rusqlite::types::FromSqlResult<Self> {
                let raw = value.as_str()?;
                raw.parse().map_err(|err: $crate::error::AppError| {
                    ::rusqlite::types::FromSqlError::Other(Box::new(::std::io::Error::new(
                        ::std::io::ErrorKind::InvalidData,
                        err.to_string(),
                    )))
                })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    string_enum! {
        /// 仅用于验证宏行为的示例词表。
        pub enum Demo {
            Alpha => "alpha",
            Beta => "beta",
        }
    }

    #[test]
    fn roundtrips_the_canonical_literal() {
        assert_eq!(Demo::Alpha.as_str(), "alpha");
        assert_eq!("beta".parse::<Demo>().unwrap(), Demo::Beta);
        assert_eq!(Demo::ALL.len(), 2);
    }

    #[test]
    fn rejects_out_of_vocabulary_values_instead_of_falling_back() {
        let err = "gamma".parse::<Demo>().unwrap_err();
        assert!(matches!(err, AppError::Domain(_)));
        assert!(err.to_string().contains("gamma"));
    }

    #[test]
    fn serialises_to_the_canonical_literal() {
        assert_eq!(serde_json::to_string(&Demo::Beta).unwrap(), "\"beta\"");
        assert_eq!(
            serde_json::from_str::<Demo>("\"alpha\"").unwrap(),
            Demo::Alpha
        );
    }
}
