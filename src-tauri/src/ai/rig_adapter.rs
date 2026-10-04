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

use futures_util::StreamExt;
use rig::completion::CompletionModel as _;
use rig::providers::openai;

use crate::ai::agents::AgentRole;
use crate::ai::config::AiConfig;
use crate::ai::provider::{is_streaming_only, mark_streaming_only};
use crate::ai::runtime::{clip_text, parse_action, tools_manual, AgentStep};
use crate::ai::tools;
use crate::error::{AppError, AppResult};
use crate::events::{RunEvent, RunSink};
use rusqlite::Connection;
use serde_json::json;
use std::pin::pin;

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
    /// 真实输入 token 数（来自 rig 的 `usage`；未上报为 0）。
    pub input_tokens: u64,
    /// 真实输出 token 数。
    pub output_tokens: u64,
    /// 重试次数（非空体回退流式的额外一次；正常为 0）。
    pub retries: u32,
    /// 工具调用步骤（Golden 对比：与 Legacy `AgentRun.steps` 对齐）。
    pub steps: Vec<AgentStep>,
    /// 实际执行轮数。
    pub rounds: usize,
}

/// 非流式补全的尝试阶梯（PERF-05）：`(max_tokens, disable_thinking)`。
///
/// 与 `OpenAiProvider::complete` 同思路：推理型模型会把输出预算花在 reasoning
/// 上，`content` 恒为空（`finish_reason=length`）。于是先原样、再提高预算、
/// 最后关闭思考；整条阶梯都空才回退流式。
///
/// 此前 rig adapter 只有固定 `max_tokens = 8_192` 一档：chunk 一多（候选输出量大）
/// 就必然空 content，于是每次都白付一次重试——实测某次 23 chunk 抽取
/// `retries = 1`，成本直接翻倍（¥0.24 → ¥0.48）。
const NON_STREAM_LADDER: [(u64, bool); 3] = [
    (8_192, false),
    (32_768, false),
    (32_768, true),
];

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
        // 与自研 provider 相同的「流式专用端点」记忆（M14 热修复用）：
        // DeepSeek 推理端点等非流式恒为空体，只有流式才有正文。
        let stream_key = format!("{}|{}", self.config.base_url, self.config.model);
        // PERF-05：三条路径都带回真实 usage（流式从终止记录读）。
        let (text, usage, retries) = if is_streaming_only(&stream_key) {
            let (text, usage) = self.stream_collect(&request).await?;
            (text, usage, 0)
        } else {
            let mut resolved: Option<(String, rig::completion::Usage)> = None;
            let mut attempts = 0u32;
            for (budget, disable_thinking) in NON_STREAM_LADDER {
                let (text, usage) =
                    self.complete_once_inner(&request, budget, disable_thinking)
                        .await?;
                attempts += 1;
                if !text.trim().is_empty() {
                    resolved = Some((text, usage));
                    break;
                }
                crate::log_warn!(
                    "rig 非流式返回空 content（max_tokens={budget} disable_thinking={disable_thinking}），升级重试"
                );
            }
            match resolved {
                Some((text, usage)) => (text, usage, attempts - 1),
                None => {
                    crate::log_warn!("rig 非流式阶梯全部为空，标记流式专用并回退");
                    mark_streaming_only(&stream_key);
                    let (text, usage) = self.stream_collect(&request).await?;
                    (text, usage, attempts)
                }
            }
        };

        Ok(AgentResult {
            answer: text,
            model: self.config.model.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            retries,
            steps: Vec::new(),
            rounds: 1,
        })
    }

    /// 非流式单轮补全，返回纯文本 + 真实用量（rig 的 `usage`）。
    async fn complete_once_inner(
        &self,
        request: &AgentRequest,
        max_tokens: u64,
        disable_thinking: bool,
    ) -> AppResult<(String, rig::completion::Usage)> {
        let client = self.completions_client()?;
        let model = openai::GenericCompletionModel::new(client, self.config.model.clone());
        let mut builder = model
            .completion_request(request.goal.clone())
            .preamble(request.system.clone())
            .temperature(0.3)
            .max_tokens(max_tokens);
        if disable_thinking {
            // 供应商特有字段，只在最后一档附带：严格端点遇到未知参数会直接 4xx。
            builder = builder.additional_params(serde_json::json!({ "enable_thinking": false }));
        }
        let response = builder
            .send()
            .await
            .map_err(|err| AppError::Internal(format!("rig 补全失败：{err}")))?;
        Ok((join_choice_text(response.choice), response.usage))
    }

    /// 流式补全：逐帧收集文本（推理模型的 reasoning 帧被自然跳过），
    /// 并返回该次调用的**真实 token 用量**。
    ///
    /// PERF-05 修正：rig 0.42 的流在终止记录（`StreamFinal`）里**确实带 usage**，
    /// 并提供 `StreamingCompletionResponse::usage()`。此前这里只收文本、把 usage
    /// 丢了，于是「非流式空体 → 回退流式」之后整条链路的 token 用量全是 0——
    /// 表现为「chunk 一多就看不到 token 用量」。现在读完流后从流上读取。
    async fn stream_collect(
        &self,
        request: &AgentRequest,
    ) -> AppResult<(String, rig::completion::Usage)> {
        let client = self.completions_client()?;
        let model = openai::GenericCompletionModel::new(client, self.config.model.clone());
        let stream = model
            .completion_request(request.goal.clone())
            .preamble(request.system.clone())
            .temperature(0.3)
            .max_tokens(32_768)
            .stream()
            .await
            .map_err(|err| AppError::Internal(format!("rig 流式请求失败：{err}")))?;
        let mut stream = pin!(stream);
        let mut text = String::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(rig::streaming::StreamedAssistantContent::Text(delta)) => {
                    text.push_str(&delta.text);
                }
                // 工具调用 / 推理帧对纯补全无意义，跳过。
                Ok(_) => {}
                Err(err) => return Err(AppError::Internal(format!("rig 流式中断：{err}"))),
            }
        }
        // 流已 drain 完，终止记录里的真实用量可读（缺失时 rig 返回零值哨兵）。
        let usage = stream.as_ref().get_ref().usage();
        Ok((text, usage))
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

    /// Golden 对比（迁移计划第十六节）：同一目标分别跑 Legacy runtime
    /// 与 Rig 适配层，对比工具序列与最终答案——**行为不漂移**才切换调用方。
    /// `cargo test --lib golden -- --ignored --nocapture`
    #[test]
    #[ignore = "需要真实 API Key，仅手动运行"]
    fn golden_legacy_vs_rig() {
        let key = std::env::var("WIKIYA_API_KEY").unwrap_or_default();
        if key.trim().is_empty() {
            eprintln!("跳过：未设置 WIKIYA_API_KEY");
            return;
        }
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::db::apply_migrations(&mut conn).unwrap();
        let goal = "查询知识库中关于 Rust 的内容并给出结论；若知识库为空，如实说明。";
        let role = AgentRole::Knowledge;

        // Legacy：自研 ReAct 循环。
        let legacy = crate::ai::runtime::run(&conn, role, goal, "golden-legacy", None)
            .expect("legacy runtime 应成功");
        // Rig：适配层循环（同一协议、同一白名单、同一轮数上限）。
        let config = AiConfig {
            api_key: Some(key),
            base_url: std::env::var("WIKIYA_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            model: std::env::var("WIKIYA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            embedding_model: "text-embedding-3-small".into(),
            token_budget: 4000,
            enabled: true,
        };
        let rig_result = RigAdapter::new(config)
            .run_react_blocking(&conn, role, goal, "golden-rig", None)
            .expect("rig runtime 应成功");

        // 行为对比（人工评审工具序列；自动断言基本健全性）。
        eprintln!(
            "legacy: {} 轮，工具 {:?}",
            legacy.rounds,
            legacy
                .steps
                .iter()
                .map(|s| s.tool.clone())
                .collect::<Vec<_>>()
        );
        eprintln!(
            "rig:    {} 轮，工具 {:?}",
            rig_result.rounds,
            rig_result
                .steps
                .iter()
                .map(|s| s.tool.clone())
                .collect::<Vec<_>>()
        );
        eprintln!("legacy answer: {}", clip_text(&legacy.answer, 200));
        eprintln!("rig answer:    {}", clip_text(&rig_result.answer, 200));
        assert!(!legacy.answer.trim().is_empty());
        assert!(!rig_result.answer.trim().is_empty());
        // 两者都应在白名单内行动（无越权工具）。
        for step in legacy.steps.iter().chain(rig_result.steps.iter()) {
            assert!(
                tools::ToolName::from_str(&step.tool).is_some(),
                "越权工具 {}",
                step.tool
            );
        }
    }
}

/// 轮数上限（与 Legacy runtime 一致——Golden 对比的行为基线之一）。
pub const MAX_ROUNDS: usize = 8;
const TOOL_OUTPUT_LIMIT: usize = 1600;

impl RigAdapter {
    /// 多轮 Agent 循环（PR2，与 Legacy `runtime::run` 行为对齐）：
    /// 每轮 rig 补全 → 解析 JSON 动作 → 白名单工具执行（Policy 闸门）
    /// → 输出回填 → 直到 final / 轮数上限。全程发统一 RunEvent。
    pub fn run_react_blocking(
        &self,
        conn: &Connection,
        role: AgentRole,
        goal: &str,
        run_id: &str,
        sink: Option<&RunSink>,
    ) -> AppResult<AgentResult> {
        self.require_key()?;
        let policy = role.policy();
        let system = format!("{}\n\n{}", role.system_prompt(), tools_manual());
        let mut user = format!("目标：{goal}");
        let mut steps: Vec<AgentStep> = Vec::new();
        // PR-05：跨轮累计真实用量（每轮 rig 响应的 usage）。
        let mut acc_input: u64 = 0;
        let mut acc_output: u64 = 0;

        let notify = |event: RunEvent| {
            if let Some(sink) = sink {
                sink(&event);
            }
        };
        notify(RunEvent::Started {
            run_id: run_id.to_string(),
            run_type: crate::domain::run::RunType::Agent,
        });

        for round in 1..=MAX_ROUNDS {
            let final_hint = if round == MAX_ROUNDS {
                "\n\n（这是最后一轮：不要调用工具，直接输出 {\"action\":\"final\",\"answer\":\"...\"}。）"
            } else {
                ""
            };
            let user_now = format!("{user}{final_hint}");
            let raw = block_on(async {
                let client = self.completions_client()?;
                let model = openai::GenericCompletionModel::new(client, self.config.model.clone());
                model
                    .completion_request(user_now)
                    .preamble(system.clone())
                    .temperature(0.3)
                    .max_tokens(8_192)
                    .send()
                    .await
                    .map_err(|err| AppError::Internal(format!("rig 补全失败：{err}")))
            })?;
            acc_input += raw.usage.input_tokens;
            acc_output += raw.usage.output_tokens;
            let text = join_choice_text(raw.choice);
            if text.trim().is_empty() {
                return Err(AppError::Internal("rig 模型未返回任何文本".into()));
            }

            match parse_action(&text)? {
                crate::ai::runtime::Action::Final { answer } => {
                    notify(RunEvent::Completed {
                        run_id: run_id.to_string(),
                    });
                    return Ok(AgentResult {
                        answer,
                        model: self.config.model.clone(),
                        input_tokens: acc_input,
                        output_tokens: acc_output,
                        retries: 0,
                        steps,
                        rounds: round,
                    });
                }
                crate::ai::runtime::Action::Tool { tool, args } => {
                    notify(RunEvent::ToolCalled {
                        run_id: run_id.to_string(),
                        tool: tool.clone(),
                        summary: clip_text(&args.to_string(), 200),
                    });
                    let name = tools::ToolName::from_str(&tool).ok_or_else(|| {
                        AppError::Internal(format!("模型请求了白名单之外的工具 `{tool}`"))
                    })?;
                    let output = tools::execute(conn, name, &args, &policy)
                        .and_then(|out| Ok(json!(out.render())))
                        .unwrap_or_else(|err| json!({ "error": err.to_string() }));
                    let ok = output.get("error").is_none();
                    let summary = clip_text(&output.to_string(), TOOL_OUTPUT_LIMIT);
                    steps.push(AgentStep {
                        tool: tool.clone(),
                        args: args.clone(),
                        summary: summary.clone(),
                    });
                    notify(RunEvent::ToolCompleted {
                        run_id: run_id.to_string(),
                        tool: tool.clone(),
                        ok,
                        summary: summary.clone(),
                    });
                    user.push_str(&format!(
                        "\n\n[第 {round} 轮：工具 `{tool}` 输出]\n{summary}\n\n\
                         请继续：要么调用下一个工具，要么给出 final 答案。"
                    ));
                }
            }
        }

        notify(RunEvent::Failed {
            run_id: run_id.to_string(),
            error: format!("Agent 在 {MAX_ROUNDS} 轮内未给出最终答案"),
        });
        Err(AppError::Internal(format!(
            "Agent 在 {MAX_ROUNDS} 轮内未给出最终答案"
        )))
    }
}

#[cfg(test)]
mod ladder_tests {
    use super::*;

    /// PERF-05：升级阶梯必须「先原样、再提高预算、最后关闭思考」。
    ///
    /// 顺序有意义：`enable_thinking` 是供应商特有字段，严格端点遇到未知参数会
    /// 直接 4xx，所以只能放在最后一档兜底；提高预算放在中间档，因为推理模型
    /// 常把预算花在 reasoning 上导致 content 为空。
    #[test]
    fn ladder_escalates_budget_before_disabling_thinking() {
        assert_eq!(NON_STREAM_LADDER[0], (8_192, false), "第一档必须原样");
        assert_eq!(
            NON_STREAM_LADDER[1],
            (32_768, false),
            "第二档应提高预算但不动思考模式"
        );
        assert_eq!(
            NON_STREAM_LADDER[2],
            (32_768, true),
            "关闭思考只能作为最后一档"
        );
        // 预算必须单调不减
        assert!(NON_STREAM_LADDER[0].0 <= NON_STREAM_LADDER[1].0);
        assert!(NON_STREAM_LADDER[1].0 <= NON_STREAM_LADDER[2].0);
    }
}
