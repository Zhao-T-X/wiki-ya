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
    /// OpenAI 兼容接口的 API Key；为空表示未启用 AI。
    pub api_key: Option<String>,
    /// 接口基址，默认官方 `https://api.openai.com/v1`。
    pub base_url: String,
    /// 模型名，默认 `gpt-4o-mini`。
    pub model: String,
    /// 向量化模型名，默认 `text-embedding-3-small`。
    pub embedding_model: String,
    /// Ask/Research 的上下文预算（token）。设置页可调，默认 4000。
    pub token_budget: usize,
    /// 是否真的可用：只有拿到 Key 才算启用。
    pub enabled: bool,
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
        const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
        const DEFAULT_MODEL: &str = "gpt-4o-mini";
        const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

        let api_key = settings_repository::get_setting(conn, KEY_API_KEY)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty());

        let resolve = |key: &str, env: Option<String>, default: &str| -> String {
            if let Ok(Some(value)) = settings_repository::get_setting(conn, key) {
                if !value.trim().is_empty() {
                    return value;
                }
            }
            env.unwrap_or_else(|| default.to_string())
        };

        let base_url = resolve(KEY_BASE_URL, std::env::var("WIKIYA_BASE_URL").ok(), DEFAULT_BASE_URL);
        let model = resolve(KEY_MODEL, std::env::var("WIKIYA_MODEL").ok(), DEFAULT_MODEL);
        let embedding_model = resolve(
            KEY_EMBEDDING_MODEL,
            std::env::var("WIKIYA_EMBEDDING_MODEL").ok(),
            DEFAULT_EMBEDDING_MODEL,
        );

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
            token_budget,
            enabled,
        }
    }
}
