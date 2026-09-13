//! Document —— 原始文档，系统的 Source of Truth。
//!
//! Rule 1：**Raw is immutable.** 结构化知识永远不能覆盖原文。
//! 因此 Document 没有「由 Claim 反写」的路径，唯一的写操作是用户显式编辑，
//! 且编辑会改变 `content_hash`，从而让幂等键重新生效。

use crate::domain::common::ids::DocumentId;
use crate::domain::common::Timestamp;
use crate::error::{AppError, AppResult};
use crate::string_enum;

string_enum! {
    /// 文档来源类型。
    ///
    /// `note` / `markdown` / `text` 来自参考实现；
    /// `html` 来自参考实现的导入白名单；
    /// `web_clip` 与 `pdf` 是 PRD §5/§6 预留的入口，
    /// **先占位不进枚举之外的枚举**——这样以后实现导入时不需要改索引与迁移。
    pub enum SourceType {
        Note => "note",
        Markdown => "markdown",
        Text => "text",
        Html => "html",
        WebClip => "web_clip",
        Pdf => "pdf",
    }
}

impl SourceType {
    pub const DEFAULT: SourceType = SourceType::Note;

    /// 该类型是否已在当前版本支持导入。
    ///
    /// UI 用它决定下拉项是否可选，避免出现"能选但不能用"的假功能。
    pub fn is_import_supported(&self) -> bool {
        matches!(
            self,
            SourceType::Note | SourceType::Markdown | SourceType::Text | SourceType::Html
        )
    }

    /// 由文件扩展名推断来源类型（导入路径）。
    pub fn from_extension(extension: &str) -> AppResult<SourceType> {
        let key = extension.trim().trim_start_matches('.').to_ascii_lowercase();
        match key.as_str() {
            "md" | "markdown" => Ok(SourceType::Markdown),
            "txt" | "text" => Ok(SourceType::Text),
            "html" | "htm" => Ok(SourceType::Html),
            other => Err(AppError::Invalid(format!(
                "不支持的文件类型：.{other}（当前支持 md / markdown / txt / text / html / htm）"
            ))),
        }
    }
}

/// 原始文档。
#[derive(Debug, Clone)]
pub struct Document {
    pub id: DocumentId,
    pub title: String,
    pub content: String,
    /// 内容 SHA-256，导入幂等键（INV-02）。
    pub content_hash: String,
    pub source_type: SourceType,
    pub source_uri: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Document {
    /// 校验并归一化用户输入。
    pub fn validate(title: &str, content: &str) -> AppResult<(String, String)> {
        let title = title.trim();
        if title.is_empty() {
            return Err(AppError::Invalid("标题不能为空".into()));
        }
        if content.trim().is_empty() {
            return Err(AppError::Invalid("内容不能为空".into()));
        }
        Ok((title.to_string(), content.to_string()))
    }

    /// 内容指纹（INV-02 的实现手段）。
    pub fn content_hash(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let digest = hasher.finalize();
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable_and_distinguishing() {
        let a = Document::content_hash("Rust supports async fn in trait.");
        let b = Document::content_hash("Rust supports async fn in trait.");
        let c = Document::content_hash("Rust does not support async fn in trait.");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn blank_documents_are_rejected() {
        assert!(Document::validate("", "body").is_err());
        assert!(Document::validate("title", "   ").is_err());
        assert_eq!(
            Document::validate("  Title  ", "body").unwrap().0,
            "Title"
        );
    }

    #[test]
    fn extensions_map_to_supported_sources_only() {
        assert_eq!(SourceType::from_extension(".MD").unwrap(), SourceType::Markdown);
        assert_eq!(SourceType::from_extension("htm").unwrap(), SourceType::Html);
        assert!(SourceType::from_extension("pdf").is_err());
    }

    #[test]
    fn reserved_source_types_are_not_yet_importable() {
        assert!(SourceType::Note.is_import_supported());
        assert!(!SourceType::Pdf.is_import_supported());
        assert!(!SourceType::WebClip.is_import_supported());
    }
}
