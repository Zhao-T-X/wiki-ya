//! Rig Agent Runtime 适配层（M14 PR1）。
//!
//! 边界（迁移计划第一步，行动计划第十六节）：wiki-ya 定义稳定接口，
//! **Rig 只在本文件内部出现**——业务层（application/ai/runtime.rs）
//! 永不直接依赖 Rig，未来 Rig 升级或换底座不拖累业务层。
//!
//! PR1 只做最小闭环（单轮补全 smoke）；`AgentRunner` / Hook / 工具
//! 循环迁移在 PR2-PR3（迁移计划第二、三阶段）。
//!
//! PoC（poc/rig-core 分支）已验证的事实，本文件直接沿用：
//! 1. rig 0.42 默认走 OpenAI **Responses API**——OpenAI 兼容端点
//!    必须显式 `.completions_api()` 切回 `/chat/completions`；
//! 2. rig 全异步，而调用方（spawn_blocking）是同步世界——用临时
//!    current-thread tokio runtime 桥接；
//! 3. 配置从 `AiConfig`（DB 加密存储）出发，不走 rig 的 `.env` 约定。

use rig::completion::CompletionModel as _;
use rig::providers::openai;

use crate::ai::config::AiConfig;
use crate::error::{AppError, AppResult};

/// 一次 Agent 执行请求（wiki-ya 稳定契约，对齐现有 `AgentRun` 语义；
/// PR2 起扩展 tools / policy / parent_run_id 等字段）。
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub goal: String,
    pub system: String,
    pub run_id: String,
}

/// 一次 Agent 执行结果（PR2 起扩展 steps / rounds）。
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub answer: String,
    pub model: String,
}

/// Rig 实现的 Agent Runtime 适配器。
pub struct RigAdapter {
    config: AiConfig,
}

impl RigAdapter {
    pub fn new(config: AiConfig) -> Self {
        Self { config }
    }

    /// 构造 rig 的 Chat Completions 客户端（显式避开 0.42 默认的
    /// Responses API——Ollama / vLLM / DashScope 等兼容端点不支持它）。
    fn completions_client(&self) -> AppResult<openai::CompletionsClient> {
        let key = self
            .config
            .api_key
            .clone()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| AppError::Internal("AI 未启用：未配置 API Key。".into()))?;
        let client = openai::Client::builder()
            .api_key(key)
            .base_url(self.config.base_url.clone())
            .build()
            .map_err(|err| AppError::Internal(format!("rig client 构建失败：{err}")))?
            .completions_api();
        Ok(client)
    }

    fn require_key(&self) -> AppResult<()> {
        if !self.config.enabled {
            return Err(AppError::Internal(
                "AI 未启用：未配置 WIKIYA_API_KEY。配置后重启应用即可启用。".into(),
            ));
        }
        Ok(())
    }

    /// 同步执行一轮补全（供 spawn_blocking 调用方；内部桥接 rig 异步）。
    pub fn run_blocking(&self, request: AgentRequest) -> AppResult<AgentResult> {
        self.require_key()?;
        block_on(self.complete_once(request))
    }

    async fn complete_once(&self, request: AgentRequest) -> AppResult<AgentResult> {
        let client = self.completions_client()?;
        let model = openai::GenericCompletionModel::new(client, self.config.model.clone());
        let response = model
            .completion_request(request.goal)
            .preamble(request.system)
            .temperature(0.3)
            .max_tokens(8_192)
            .send()
            .await
            .map_err(|err| AppError::Internal(format!("rig 补全失败：{err}")))?;

        Ok(AgentResult {
            answer: join_choice_text(response.choice),
            model: self.config.model.clone(),
        })
    }
}

/// 从 rig 的响应 choice 里拼接纯文本。
fn join_choice_text(choice: Vec<rig::completion::AssistantContent>) -> String {
    let mut text = String::new();
    for part in choice {
        if let rig::completion::AssistantContent::Text(t) = part {
            text.push_str(&t.text);
        }
    }
    text
}

/// async → sync 桥接：Provider/Adapter 契约是同步的（调用点在
/// `spawn_blocking` 线程，没有 Tokio 上下文），用临时 current-thread
/// runtime 执行，构建开销微秒级。
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rig adapter: 构建 tokio runtime");
    rt.block_on(fut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_key() -> AiConfig {
        AiConfig {
            api_key: Some("sk-test".into()),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            embedding_model: "text-embedding-3-small".into(),
            token_budget: 4000,
            enabled: true,
        }
    }

    #[test]
    fn rig_adapter_reports_enabled_with_key() {
        // 有 key：require_key 通过（网络可达性不纳入单元测试）。
        let adapter = RigAdapter::new(config_with_key());
        assert!(adapter.require_key().is_ok());
    }

    #[test]
    fn rig_adapter_disabled_without_key() {
        let mut config = config_with_key();
        config.api_key = None;
        config.enabled = false;
        let adapter = RigAdapter::new(config);
        let err = adapter
            .run_blocking(AgentRequest {
                goal: "ping".into(),
                system: "test".into(),
                run_id: "r1".into(),
            })
            .unwrap_err();
        assert!(err.to_string().contains("未配置"));
    }

    /// 真实调用冒烟（需要 WIKIYA_API_KEY / WIKIYA_BASE_URL / WIKIYA_MODEL）：
    /// `cargo test --lib rig_live -- --ignored --nocapture`
    #[test]
    #[ignore = "需要真实 API Key，仅手动运行"]
    fn rig_live_smoke() {
        let key = std::env::var("WIKIYA_API_KEY").unwrap_or_default();
        if key.trim().is_empty() {
            eprintln!("跳过：未设置 WIKIYA_API_KEY");
            return;
        }
        let config = AiConfig {
            api_key: Some(key),
            base_url: std::env::var("WIKIYA_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            model: std::env::var("WIKIYA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            embedding_model: "text-embedding-3-small".into(),
            token_budget: 4000,
            enabled: true,
        };
        let adapter = RigAdapter::new(config);
        let result = adapter
            .run_blocking(AgentRequest {
                goal: "用一句话介绍 SQLite。".into(),
                system: "你是测试助手，回答保持一句话。".into(),
                run_id: "smoke".into(),
            })
            .expect("rig live smoke 应成功");
        assert!(!result.answer.trim().is_empty());
        println!("[rig adapter] {}", result.answer);
    }
}
