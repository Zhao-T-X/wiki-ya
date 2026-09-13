//! 统一错误类型。
//!
//! 跨层契约：Domain / Application / Infrastructure / Commands 一律使用
//! [`AppError`]，不各自定义错误类型，也不把 `rusqlite::Error` 泄漏到上层
//! （TDD §86：Domain 不知道 SQLite）。
//!
//! 序列化给前端时固定为 `{ code, message }`：`code` 供程序判断，
//! `message` 供展示。前端**不得**解析 `message` 文本做逻辑判断。

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("数据库错误：{0}")]
    Database(String),

    #[error("未找到：{0}")]
    NotFound(String),

    #[error("输入不合法：{0}")]
    Invalid(String),

    /// 幂等键冲突（如重复导入同一 content_hash）。
    #[error("冲突：{0}")]
    Conflict(String),

    /// 领域不变量被违反（枚举越界、谓词未注册、关系方向非法等）。
    #[error("违反领域规则：{0}")]
    Domain(String),

    #[error("内部错误：{0}")]
    Internal(String),
}

impl AppError {
    /// 稳定的机器可读错误码，前端据此做分支，不要依赖文案。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Database(_) => "DATABASE_ERROR",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Invalid(_) => "INVALID_INPUT",
            AppError::Conflict(_) => "CONFLICT",
            AppError::Domain(_) => "DOMAIN_RULE_VIOLATION",
            AppError::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        match &err {
            // UNIQUE 约束是幂等的实现手段（INV-02/INV-03/INV-04/INV-07），
            // 因此必须映射为 Conflict 而不是笼统的 Database 错误。
            rusqlite::Error::SqliteFailure(e, _)
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                let text = err.to_string();
                if text.contains("UNIQUE") {
                    AppError::Conflict(text)
                } else {
                    AppError::Domain(text)
                }
            }
            _ => AppError::Database(err.to_string()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Internal(format!("JSON 序列化失败：{err}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(format!("IO 失败：{err}"))
    }
}

/// 全项目统一返回类型。
pub type AppResult<T> = Result<T, AppError>;
