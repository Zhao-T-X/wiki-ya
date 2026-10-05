//! AI 运行时配置。
//!
//! 读取优先级：应用内持久化设置（settings 表）> 环境变量 > 默认值。
//! 这样用户既可在设置页 UI 配置（持久化、保存即生效），也可在 shell / .env 中预设，
//! 读取入口统一在此，单一事实来源。

use rusqlite::Connection;

use crate::infrastructure::settings_repository;

/// AI 运行配置。
#[derive(Debug, Clone)]
pub struct AiConfig {
    /// 对话接口的 API Key；为空表示未启用 AI。
    pub api_key: Option<String>,
    /// 对话接口基址，默认官方 `https://api.openai.com/v1`。
    pub base_url: String,
    /// 对话模型名，默认 `gpt-4o-mini`。
    pub model: String,
    /// 向量化模型名，默认 `text-embedding-3-small`；`local:` 前缀表示本机推理。
    pub embedding_model: String,
    /// 向量接口基址；**空字符串表示复用 [`Self::base_url`]**。
    ///
    /// 为什么需要独立：不少服务商（含 DeepSeek）**只提供 chat/completions，
    /// 没有 /embeddings 端点**。共用一个基址就等于把向量化请求发给不支持的
    /// 端点，只能靠本机推理兜底。
    pub embedding_base_url: String,
    /// 向量接口的 API Key；`None` 表示复用 [`Self::api_key`]。
    pub embedding_api_key: Option<String>,
    /// Ask/Research 的上下文预算（token）。设置页可调，默认 4000。
    pub token_budget: usize,
    /// 是否真的可用：只有拿到 Key 才算启用。
    pub enabled: bool,
}

impl AiConfig {
    /// 向量请求该打到哪个基址：独立配置优先，空则回退到对话基址。
    pub fn embedding_endpoint(&self) -> &str {
        let trimmed = self.embedding_base_url.trim();
        if trimmed.is_empty() {
            &self.base_url
        } else {
            trimmed
        }
    }

    /// 向量请求该用哪把 Key：独立配置优先，`None` 则回退到对话 Key。
    pub fn embedding_key(&self) -> Option<&str> {
        self.embedding_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .or(self.api_key.as_deref())
    }

    /// 远程向量化是否具备可用条件（缺 Key / 缺基址即不可用）。
    ///
    /// 本机推理（`local:` 前缀）不需要这些，故不参与判断。
    pub fn remote_embedding_ready(&self) -> bool {
        !self.embedding_model.trim().is_empty()
            && !self.embedding_endpoint().trim().is_empty()
            && self.embedding_key().is_some()
    }
}

impl AiConfig {
    /// 从「持久化设置 > 环境变量 > 默认值」三层解析配置。
    ///
    /// 设置页写入的 `settings` 表优先于环境变量；两者都缺时回退到默认值。
    /// 只有拿到 API Key 才算启用 AI。
    pub fn from_settings(conn: &Connection) -> Self {
        const KEY_API_KEY: &str = "ai.api_key";
        const KEY_BASE_URL: &str = "ai.base_url";
        const KEY_MODEL: &str = "ai.model";
        const KEY_EMBEDDING_MODEL: &str = "ai.embedding_model";
        /// 向量端点（PERF-10）：**空 = 复用 `ai.base_url`**，故默认值是空串。
        const KEY_EMBEDDING_BASE_URL: &str = "ai.embedding_base_url";
        const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
        const DEFAULT_MODEL: &str = "gpt-4o-mini";
        const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

        // SEC-001：API Key 以 AES-256-GCM 密文存在 SQLite（`ai.api_key.enc`）；
        // 为兼容尚未迁移完成的旧库，再回退到 `ai.api_key` 明文；最后才是环境变量。
        // 读取失败一律视为「没有」，绝不让 AI 因为存储层异常而崩溃。
        let api_key = crate::infrastructure::secrets::load_api_key(conn)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                settings_repository::get_setting(conn, KEY_API_KEY)
                    .ok()
                    .flatten()
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| {
                std::env::var("WIKIYA_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });

        let resolve = |key: &str, env: Option<String>, default: &str| -> String {
            if let Ok(Some(value)) = settings_repository::get_setting(conn, key) {
                if !value.trim().is_empty() {
                    return value;
                }
            }
            env.unwrap_or_else(|| default.to_string())
        };

        // 解析**允许留空**的配置（普通注释而非文档注释：闭包不是 item）。
        //
        // 与 `resolve` 的关键区别：空串在这里是**有意义的值**（表示"复用对话侧
        // 配置"），不能当成"未设置"而被默认值或环境变量顶掉。
        let resolve_blankable = |key: &str, env: Option<String>| -> String {
            if let Ok(Some(value)) = settings_repository::get_setting(conn, key) {
                return value.trim().to_string();
            }
            env.unwrap_or_default().trim().to_string()
        };

        let base_url = resolve(
            KEY_BASE_URL,
            std::env::var("WIKIYA_BASE_URL").ok(),
            DEFAULT_BASE_URL,
        );
        let model = resolve(KEY_MODEL, std::env::var("WIKIYA_MODEL").ok(), DEFAULT_MODEL);
        let embedding_model = resolve(
            KEY_EMBEDDING_MODEL,
            std::env::var("WIKIYA_EMBEDDING_MODEL").ok(),
            DEFAULT_EMBEDDING_MODEL,
        );

        // ---- 向量侧独立配置（PERF-10：与对话彻底分开）----
        //
        // 端点与密钥都**允许留空**，留空即复用对话侧的取值。这样：
        // - 单服务用户（OpenAI 一把 Key 全包）零配置，不受新增字段打扰；
        // - DeepSeek 聊天 + 硅基流动向量的组合可以各自指向。
        let embedding_base_url = resolve_blankable(
            KEY_EMBEDDING_BASE_URL,
            std::env::var("WIKIYA_EMBEDDING_BASE_URL").ok(),
        );
        let embedding_api_key = crate::infrastructure::secrets::load_named_secret(
            conn,
            crate::infrastructure::secrets::EMBEDDING_API_KEY_NAME,
        )
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("WIKIYA_EMBEDDING_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });

        // 预算：设置 > 环境变量 > 默认；非法值（0 / 非数字）诚实回退默认。
        const KEY_TOKEN_BUDGET: &str = "ai.token_budget";
        const DEFAULT_TOKEN_BUDGET: usize = 4000;
        let token_budget = settings_repository::get_setting(conn, KEY_TOKEN_BUDGET)
            .ok()
            .flatten()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .or_else(|| {
                std::env::var("WIKIYA_TOKEN_BUDGET")
                    .ok()
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .filter(|budget| *budget > 0)
            .unwrap_or(DEFAULT_TOKEN_BUDGET);

        let enabled = api_key.is_some();
        AiConfig {
            api_key,
            base_url,
            model,
            embedding_model,
            embedding_base_url,
            embedding_api_key,
            token_budget,
            enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AiConfig {
        AiConfig {
            api_key: Some("chat-key".into()),
            base_url: "https://api.deepseek.com/v1".into(),
            model: "deepseek-chat".into(),
            embedding_model: "text-embedding-3-small".into(),
            embedding_base_url: String::new(),
            embedding_api_key: None,
            token_budget: 4000,
            enabled: true,
        }
    }

    /// 留空即复用对话侧：单服务用户（OpenAI 一把 Key）零配置。
    #[test]
    fn blank_embedding_settings_fall_back_to_chat_side() {
        let cfg = config();
        assert_eq!(cfg.embedding_endpoint(), "https://api.deepseek.com/v1");
        assert_eq!(cfg.embedding_key(), Some("chat-key"));
    }

    /// 独立配置优先：DeepSeek 聊天 + 另一家向量化。
    #[test]
    fn dedicated_embedding_settings_win() {
        let mut cfg = config();
        cfg.embedding_base_url = " https://api.siliconflow.cn/v1 ".into();
        cfg.embedding_api_key = Some("  emb-key ".into());
        assert_eq!(cfg.embedding_endpoint(), "https://api.siliconflow.cn/v1");
        assert_eq!(cfg.embedding_key(), Some("emb-key"), "应去掉首尾空白");
    }

    /// 显式清空向量密钥（空串）应回退到对话密钥，而不是变成"没有密钥"。
    #[test]
    fn empty_embedding_key_falls_back_instead_of_disabling() {
        let mut cfg = config();
        cfg.embedding_api_key = Some("   ".into());
        assert_eq!(cfg.embedding_key(), Some("chat-key"));
    }

    /// 远程向量化缺料就该判为不可用——否则会等到检索时才发现。
    #[test]
    fn remote_embedding_readiness_requires_endpoint_and_key() {
        assert!(config().remote_embedding_ready());
        let mut no_key = config();
        no_key.api_key = None;
        assert!(!no_key.remote_embedding_ready());
        let mut no_endpoint = config();
        no_endpoint.api_key = None;
        no_endpoint.base_url = String::new();
        assert!(!no_endpoint.remote_embedding_ready());
    }
}
