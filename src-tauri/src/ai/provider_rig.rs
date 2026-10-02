//! rig-core PoC（仅 `--features poc-rig` 下编译）。
//!
//! 目的：验证用 rig-core 0.42 替换自研 `provider.rs` HTTP 层的可行性。
//! 实现同一个 `Provider` trait，行为对齐 `OpenAiProvider`，便于 A/B 对比。
//!
//! PoC 结论性发现（写代码过程中确认）：
//! 1. rig 0.42 的默认客户端走 OpenAI **Responses API**（`/responses`），
//!    大多数 OpenAI 兼容端点（Ollama / vLLM / DashScope）只支持
//!    `/chat/completions` —— 必须显式 `.completions_api()` 切换。
//! 2. rig 是 async（tokio + reqwest 0.13），而我们的 `Provider` trait 是同步的
//!    （调用点都在 `spawn_blocking` 线程）。桥接用临时 current-thread runtime，
//!    每次 `complete` 构建一次，开销可忽略，但语义上是从同步世界"进"异步世界。
//! 3. 配置仍然从我们的 `AiConfig`（DB 加密存储）出发，不走 rig 的
//!    `from_env()` / `.env` 约定。

use futures_util::StreamExt;
use rig::completion::CompletionModel as _;
use rig::embeddings::EmbeddingModel as _;
use rig::providers::openai;

use crate::ai::config::AiConfig;
use crate::ai::provider::{CompletionRequest, CompletionResponse, Provider};
use crate::error::{AppError, AppResult};

/// 用 rig 实现的 OpenAI 兼容 Provider（PoC）。
pub struct RigProvider {
    config: AiConfig,
}

impl RigProvider {
    pub fn new(config: AiConfig) -> Self {
        Self { config }
    }

    /// 构造 rig 的 Chat Completions 客户端（显式避开 0.42 默认的 Responses API）。
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

    /// 构造补全模型实例。
    fn completion_model(
        &self,
    ) -> AppResult<openai::GenericCompletionModel<openai::OpenAICompletionsExt>> {
        let client = self.completions_client()?;
        Ok(openai::GenericCompletionModel::new(
            client,
            self.config.model.clone(),
        ))
    }

    fn require_key(&self) -> AppResult<()> {
        if !self.config.enabled {
            return Err(AppError::Internal(
                "AI 未启用：未配置 WIKIYA_API_KEY。配置后重启应用即可启用。".into(),
            ));
        }
        Ok(())
    }
}

impl Provider for RigProvider {
    fn name(&self) -> &'static str {
        "rig-openai-compatible"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn complete(&self, request: &CompletionRequest) -> AppResult<CompletionResponse> {
        self.require_key()?;
        let model = self.completion_model()?;
        let response = block_on(async {
            model
                .completion_request(request.user.clone())
                .preamble(request.system.clone())
                .temperature(request.temperature as f64)
                .max_tokens(request.max_tokens as u64)
                .send()
                .await
        })
        .map_err(|err| AppError::Internal(format!("rig 补全失败：{err}")))?;

        Ok(CompletionResponse {
            text: join_choice_text(response.choice),
            model: self.config.model.clone(),
        })
    }

    fn complete_streaming(
        &self,
        request: &CompletionRequest,
        on_delta: &dyn Fn(&str),
    ) -> AppResult<CompletionResponse> {
        self.require_key()?;
        let model = self.completion_model()?;

        let mut full = String::new();
        block_on(async {
            let stream = model
                .completion_request(request.user.clone())
                .preamble(request.system.clone())
                .temperature(request.temperature as f64)
                .max_tokens(request.max_tokens as u64)
                .stream()
                .await
                .map_err(|err| AppError::Internal(format!("rig 流式请求失败：{err}")))?;

            let mut stream = std::pin::pin!(stream);
            while let Some(item) = stream.next().await {
                match item {
                    Ok(rig::streaming::StreamedAssistantContent::Text(delta)) => {
                        on_delta(&delta.text);
                        full.push_str(&delta.text);
                    }
                    // 工具调用 / 推理块对纯补全场景无意义，PoC 忽略。
                    Ok(_) => {}
                    Err(err) => {
                        return Err(AppError::Internal(format!("rig 流式中断：{err}")));
                    }
                }
            }
            Ok(())
        })?;

        Ok(CompletionResponse {
            text: full,
            model: self.config.model.clone(),
        })
    }

    fn embed(&self, texts: &[String]) -> AppResult<Vec<Vec<f32>>> {
        self.require_key()?;
        // rig 0.42 要求显式维度数；text-embedding-3-small 为 1536。
        // 注意：dimensions 参数对不支持的兼容端点可能被拒绝（PoC 已知限制）。
        const NDIMS: usize = 1536;
        let client = openai::Client::builder()
            .api_key(
                self.config
                    .api_key
                    .clone()
                    .ok_or_else(|| AppError::Internal("AI 未启用".into()))?,
            )
            .base_url(self.config.base_url.clone())
            .build()
            .map_err(|err| AppError::Internal(format!("rig client 构建失败：{err}")))?;
        let model = openai::GenericEmbeddingModel::new(
            client,
            self.config.embedding_model.clone(),
            NDIMS,
        );

        let embeddings = block_on(async { model.embed_texts(texts.to_vec()).await })
            .map_err(|err| AppError::Internal(format!("rig 向量化失败：{err}")))?;

        // rig 的向量是 f64，我们的向量存储用 f32（float32 blob，TDD §36）。
        Ok(embeddings
            .into_iter()
            .map(|e| e.vec.into_iter().map(|v| v as f32).collect())
            .collect())
    }
}

/// 从 rig 的响应 choice 里拼接出纯文本。
fn join_choice_text(choice: Vec<rig::completion::AssistantContent>) -> String {
    let mut text = String::new();
    for part in choice {
        if let rig::completion::AssistantContent::Text(t) = part {
            text.push_str(&t.text);
        }
    }
    text
}

/// async → sync 桥接：`Provider` trait 是同步的（调用点在 `spawn_blocking`
/// 线程，没有 Tokio 上下文），rig 是 async。用临时 current-thread runtime 执行，
/// 构建开销微秒级，PoC 可接受。
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("PoC: 构建 tokio current-thread runtime");
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
    fn rig_provider_reports_enabled_with_key() {
        let provider = RigProvider::new(config_with_key());
        assert!(provider.enabled());
        assert_eq!(provider.name(), "rig-openai-compatible");
    }

    #[test]
    fn rig_provider_disabled_without_key() {
        let mut config = config_with_key();
        config.api_key = None;
        config.enabled = false;
        let provider = RigProvider::new(config);
        assert!(!provider.enabled());
        let request = CompletionRequest::chat("sys".into(), "user".into());
        let err = provider.complete(&request).unwrap_err();
        assert!(err.to_string().contains("未配置"));
    }

    /// 真实调用冒烟测试：需要环境变量 WIKIYA_API_KEY（可选 WIKIYA_BASE_URL /
    /// WIKIYA_MODEL）。默认 `cargo test` 会跳过（#[ignore]），手动运行：
    /// `cargo test --features poc-rig rig_live -- --ignored --nocapture`
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
        let provider = RigProvider::new(config);

        let request = CompletionRequest::chat(
            "你是一个测试助手，回答保持一句话。".into(),
            "用一句话介绍 SQLite。".into(),
        );
        let response = provider.complete(&request).expect("complete 应成功");
        assert!(!response.text.trim().is_empty());
        println!("[rig complete] {}", response.text);

        let deltas = std::cell::RefCell::new(0usize);
        let streamed = provider
            .complete_streaming(&request, &|delta| {
                *deltas.borrow_mut() += 1;
                print!("{delta}");
            })
            .expect("complete_streaming 应成功");
        println!("\n[rig streaming] 增量 {} 段，共 {} 字符",
            deltas.borrow(), streamed.text.chars().count());
        assert!(!streamed.text.trim().is_empty());
    }
}
