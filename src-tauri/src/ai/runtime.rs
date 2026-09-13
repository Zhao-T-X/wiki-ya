//! Agent 运行时（Phase 6，TDD §49–§53）。
//!
//! 最小可用的 ReAct 式循环：模型每轮输出一个 JSON 动作
//! （`{"action":"tool",...}` 调工具，或 `{"action":"final",...}` 给出最终答案），
//! 工具输出压缩后回填，直到 final 或达到轮数上限。
//!
//! 为什么用 JSON 动作协议而非供应商 function calling：
//! 对任意 OpenAI 兼容端点（含 Ollama / vLLM 等本地推理服务）都可用，
//! 且不需要扩展 Provider 的请求结构。工具调用一律经 `ai::tools` 白名单
//! 分发（TDD §50：Agent → Tool → Application Service → Domain）。

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};

use crate::ai::agents::AgentRole;
use crate::ai::config::AiConfig;
use crate::ai::provider::{default_provider, CompletionRequest};
use crate::ai::tools::{self, ToolOutput, ToolName};
use crate::error::{AppError, AppResult};
use crate::events::{AppEvent, EventSink};
use crate::infrastructure::telemetry_repository;

/// 循环轮数上限：防止工具调用无限循环烧 token。
const MAX_ROUNDS: usize = 8;

/// 给模型看的单条工具输出上限（字符）——工具输出必须短（TDD §39）。
const TOOL_OUTPUT_LIMIT: usize = 1600;

/// 一步工具调用（供 UI 展示研究过程与审计）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStep {
    pub tool: String,
    pub args: Value,
    pub summary: String,
}

/// 一次 Agent 运行的完整结果。
#[derive(Debug, Clone)]
pub struct AgentRun {
    pub answer: String,
    pub steps: Vec<AgentStep>,
    pub rounds: usize,
}

/// 运行一次 Agent 循环。
///
/// 诚实边界：工具执行失败不中断循环——错误原文作为工具输出回填给模型，
/// 让它决定换工具或如实说明「无法获取」；只有 Provider 失败或超轮数才报错。
///
/// `sink` 用于把过程事件（开始 / 思考 / token 增量 / 工具调用 / 完成）实时
/// 推给前端（TDD §53）；`None` 时静默运行，行为不变。
pub fn run(
    conn: &Connection,
    role: AgentRole,
    goal: &str,
    run_id: &str,
    sink: Option<&EventSink>,
) -> AppResult<AgentRun> {
    // Run Trace（TDD §79）：agent_runs/agent_events 落库，可回放、可审计。
    let config = AiConfig::from_settings(conn);
    let agent_run_id = telemetry_repository::start_agent_run(
        conn,
        "agent",
        Some(&format!("{role:?}")),
        Some(&config.model),
    )?;
    let started = std::time::Instant::now();

    match run_inner(conn, role, goal, run_id, sink, &agent_run_id) {
        Ok(run) => {
            let _ = telemetry_repository::finish_agent_run(
                conn,
                &agent_run_id,
                "success",
                run.steps.len() as i64,
                &serde_json::json!({ "answer_chars": run.answer.chars().count() }),
                None,
                started.elapsed().as_millis() as i64,
            );
            Ok(run)
        }
        Err(err) => {
            let _ = telemetry_repository::finish_agent_run(
                conn,
                &agent_run_id,
                "failed",
                0,
                &serde_json::json!({}),
                Some(&err.to_string()),
                started.elapsed().as_millis() as i64,
            );
            Err(err)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    conn: &Connection,
    role: AgentRole,
    goal: &str,
    run_id: &str,
    sink: Option<&EventSink>,
    agent_run_id: &str,
) -> AppResult<AgentRun> {
    let provider = default_provider(conn);
    if !provider.enabled() {
        return Err(AppError::Internal("AI 未启用：未配置 API Key。".into()));
    }

    let notify = |event: AppEvent| {
        if let Some(sink) = sink {
            sink(&event);
        }
    };
    notify(AppEvent::AgentStarted {
        run_id: run_id.to_string(),
        agent: format!("{role:?}"),
    });

    let system = format!("{}\n\n{}", role.system_prompt(), tools_manual());
    let mut user = format!("目标：{goal}");
    let mut steps: Vec<AgentStep> = Vec::new();

    for round in 1..=MAX_ROUNDS {
        let final_hint = if round == MAX_ROUNDS {
            "\n\n（这是最后一轮：不要调用工具，直接输出 {\"action\":\"final\",\"answer\":\"...\"}。）"
        } else {
            ""
        };

        notify(AppEvent::AgentThinking {
            run_id: run_id.to_string(),
            text: String::new(),
        });

        let request = CompletionRequest::chat(system.clone(), format!("{user}{final_hint}"));
        let stream_run_id = run_id.to_string();
        let response = provider.complete_streaming(&request, &|delta| {
            notify(AppEvent::TokenDelta {
                run_id: stream_run_id.clone(),
                delta: delta.to_string(),
            });
        })?;
        let action = parse_action(&response.text)?;

        match action {
            Action::Final { answer } => {
                notify(AppEvent::AgentFinished {
                    run_id: run_id.to_string(),
                    status: "success".into(),
                });
                return Ok(AgentRun {
                    answer,
                    steps,
                    rounds: round,
                });
            }
            Action::Tool { tool, args } => {
                notify(AppEvent::ToolCalled {
                    run_id: run_id.to_string(),
                    tool: tool.clone(),
                    arguments: args.clone(),
                });
                let name = ToolName::from_str(&tool).ok_or_else(|| {
                    AppError::Internal(format!("模型请求了白名单之外的工具 `{tool}`"))
                })?;
                let output = tools::execute(conn, name, &args)
                    .and_then(|out: ToolOutput| Ok(json!(out.render())))
                    .unwrap_or_else(|err| json!({ "error": err.to_string() }));

                let ok = output.get("error").is_none();
                let summary = clip_text(&output.to_string(), TOOL_OUTPUT_LIMIT);
                let _ = telemetry_repository::insert_agent_event(
                    conn,
                    agent_run_id,
                    steps.len() as i64,
                    &tool,
                    if ok { "success" } else { "failed" },
                    Some(&clip_text(&args.to_string(), 200)),
                    Some(&summary),
                    (!ok).then_some(summary.as_str()),
                );
                steps.push(AgentStep {
                    tool: tool.clone(),
                    args,
                    summary: summary.clone(),
                });
                notify(AppEvent::ToolCompleted {
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

    notify(AppEvent::AgentFinished {
        run_id: run_id.to_string(),
        status: "failed".into(),
    });
    Err(AppError::Internal(format!(
        "Agent 在 {MAX_ROUNDS} 轮内未给出最终答案"
    )))
}

enum Action {
    Final { answer: String },
    Tool { tool: String, args: Value },
}

/// 解析模型的 JSON 动作（宽松：容忍 markdown 围栏与前后解释文字）。
fn parse_action(text: &str) -> AppResult<Action> {
    let start = text.find('{').ok_or_else(|| {
        AppError::Internal(format!("模型输出中没有 JSON 动作：{}", clip_text(text, 120)))
    })?;
    let end = text.rfind('}').ok_or_else(|| {
        AppError::Internal(format!("模型输出中的 JSON 不完整：{}", clip_text(text, 120)))
    })?;
    if end < start {
        return Err(AppError::Internal("模型输出的 JSON 格式无效".into()));
    }

    let value: Value = serde_json::from_str(&text[start..=end])
        .map_err(|err| AppError::Internal(format!("模型动作 JSON 解析失败：{err}")))?;

    match value.get("action").and_then(|v| v.as_str()) {
        Some("final") => {
            let answer = value
                .get("answer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if answer.is_empty() {
                return Err(AppError::Internal("模型 final 动作的 answer 为空".into()));
            }
            Ok(Action::Final { answer })
        }
        Some("tool") => {
            let tool = value
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if tool.is_empty() {
                return Err(AppError::Internal("模型 tool 动作缺少工具名".into()));
            }
            let args = value.get("args").cloned().unwrap_or_else(|| json!({}));
            Ok(Action::Tool { tool, args })
        }
        other => Err(AppError::Internal(format!(
            "未知动作 `{:?}`（只允许 tool / final）",
            other
        ))),
    }
}

/// 工具手册：白名单、参数与 JSON 动作协议（拼进 system prompt）。
fn tools_manual() -> String {
    let tools = [
        ("search_knowledge", r#"{"query": "关键词", "limit": 8}"#),
        ("get_knowledge", r#"{"id": "<claim 或 entity id>"}"#),
        ("get_entities", r#"{"ids": ["<entity id>", "..."]}"#),
        ("get_entity", r#"{"id": "<entity id>"}"#),
        ("get_claim", r#"{"id": "<claim id>"}"#),
        ("get_evidence", r#"{"claim_id": "<claim id>"}"#),
        ("find_related", r#"{"entity_id": "<entity id>"}"#),
        ("compare_claims", r#"{"claim_a": "<id>", "claim_b": "<id>"}"#),
        ("detect_conflict", r#"{"entity_id": "<id>"}"#),
        ("propose_evolution", r#"{"claim_id": "<id>"}"#),
        (
            "request_review",
            r#"{"target_id": "<对象 id>", "reason": "理由"}"#,
        ),
    ];
    let list = tools
        .iter()
        .map(|(name, args)| format!("- `{name}`，参数：{args}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "你可以通过工具查询用户的本地知识库。可用工具（白名单之外一律不可用）：\n\
         {list}\n\n\
         动作协议：每一轮你只输出一个 JSON 对象，不要输出其它文字：\n\
         调工具：{{\"action\":\"tool\",\"tool\":\"<工具名>\",\"args\":{{...}}}}\n\
         给答案：{{\"action\":\"final\",\"answer\":\"...\"}}\n\
         规则：优先用 search_knowledge 定位，再用 get_* 下钻；\
         工具拿不到的信息要如实说明「知识库中未找到」，绝不编造。回答用中文。"
    )
}

fn clip_text(text: &str, limit: usize) -> String {
    let clipped: String = text.chars().take(limit).collect();
    if clipped.len() < text.len() {
        format!("{clipped}…（已截断）")
    } else {
        text.to_string()
    }
}
