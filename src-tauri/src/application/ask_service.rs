//! Ask 问答（Phase 6 完整版）。
//!
//! 流程：role 路由 → 检索 → 上下文编排（Context Efficiency + Cache）→ 带引用
//! 约束的流式补全 → 遥测落库（agent_runs / context_runs / context_sections）。
//! 诚实优先：AI 未启用时返回 `enabled:false`；检索不到相关上下文时，
//! 模型被要求明确说「未找到」，绝不编造答案或伪造引用（PRD + TDD §65）。

use rusqlite::Connection;
use serde_json::json;

use crate::ai::agents::AgentRole;
use crate::ai::config::AiConfig;
use crate::ai::context::{
    compile, estimate_tokens, Budget, ContextItem, ContextPack, ContextKind, LoadStrategy,
};
use crate::ai::provider::{default_provider, CompletionRequest};
use crate::application::dto::{AskRequest, AskResponse, AskSource};
use crate::application::retrieval_service::retrieve;
use crate::domain::ontology::registry;
use crate::error::AppResult;
use crate::events::{AppEvent, EventSink};
use crate::infrastructure::{db, telemetry_repository};

/// 进入编排前的检索候选上限。
const RETRIEVE_LIMIT: usize = 15;

pub fn ask(
    conn: &Connection,
    request: AskRequest,
    sink: Option<&EventSink>,
) -> AppResult<AskResponse> {
    let config = AiConfig::from_settings(conn);
    let provider = default_provider(conn);
    if !provider.enabled() {
        return Ok(AskResponse {
            question: request.question.clone(),
            answer: String::new(),
            enabled: false,
            note: Some(
                "AI 未启用：请先在 Settings → AI 运行时 配置 API Key。".into(),
            ),
            sources: Vec::new(),
            context_stats: None,
        });
    }

    let notify = |event: AppEvent| {
        if let Some(sink) = sink {
            sink(&event);
        }
    };

    // 0) role 路由：auto 时按问题类型选角色（TDD §52）。
    let role = match request.role.as_deref().unwrap_or("auto") {
        "auto" | "" => resolve_auto_role(&request.question),
        other => AgentRole::from_str(other),
    };
    let role_name = match role {
        AgentRole::Knowledge | AgentRole::Auto => "KnowledgeAgent",
        AgentRole::Research => "ResearchAgent",
        AgentRole::Curator => "CuratorAgent",
        AgentRole::Review => "ReviewAgent",
        AgentRole::Personal => "PersonalAgent",
        AgentRole::Extraction => "ExtractionAgent",
    };

    // Run Trace（TDD §79）。
    let agent_run_id =
        telemetry_repository::start_agent_run(conn, "ask", Some(role_name), Some(&config.model))?;
    let started = std::time::Instant::now();
    let run_id = request.run_id.clone().unwrap_or_default();

    if !run_id.is_empty() {
        notify(AppEvent::AgentStarted {
            run_id: run_id.clone(),
            agent: role_name.into(),
        });
    }

    let system = role.system_prompt().to_string();

    // 1) 检索候选（语义可用时融合，否则词法）。
    let passages = retrieve(conn, &request.question, RETRIEVE_LIMIT, true)?;

    // 2) 映射成 ContextItem：优先级由排名 + 检索分数决定。
    let items: Vec<ContextItem> = passages
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let priority = (idx as f32 + 1.0) * 0.5 + p.score;
            ContextItem {
                id: p.id.clone(),
                kind: p.kind,
                content: p.content.clone(),
                token_cost: estimate_tokens(&p.content),
                priority,
                strategy: LoadStrategy::Load,
                source_id: p.source_id.clone(),
                title: Some(p.title.clone()),
            }
        })
        .collect();

    // 3) 编排：预算来自设置（可调）；命中缓存时复用编译结果（INV-20/21）。
    let budget = Budget {
        max_tokens: config.token_budget,
    };
    let cache_key = context_cache_key(conn, &request.question, &config.model);
    let pack = match telemetry_repository::cache_get(conn, &cache_key)? {
        Some(cached) => serde_json::from_value::<ContextPack>(cached)
            .unwrap_or_else(|_| compile_context(items, &budget, conn, &cache_key)),
        None => compile_context(items, &budget, conn, &cache_key),
    };

    // 3b) 遥测：context_runs + context_sections（预算/压缩/裁剪可审计）。
    if let Ok(context_run_id) = telemetry_repository::insert_context_run(
        conn,
        &agent_run_id,
        role_name,
        budget.max_tokens as i64,
        pack.total_tokens as i64,
        0,
        pack.truncated,
    ) {
        for (index, item) in pack.items.iter().enumerate() {
            let _ = telemetry_repository::insert_context_section(
                conn,
                &context_run_id,
                index as i64,
                item.title.as_deref().unwrap_or(&item.id),
                policy_str(item.strategy),
                Some(&item.id),
                item.token_cost as i64,
                item.content.chars().count() as i64,
            );
        }
    }

    // 4) 组装带编号的上下文段落（编号即引用下标 [n]）。
    // 注意：编号必须与下方 `sources` 的 `index = i + 1` 使用**同一基准**，
    // 否则模型引用的 [n] 与前端 Sources 会系统性错位一位。
    let context_block = pack
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let title = item.title.clone().unwrap_or_else(|| item.id.clone());
            format!("[{}] ({title}) {content}", i + 1, content = item.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let user = if context_block.trim().is_empty() {
        format!(
            "用户问题：{}\n\n（知识库中未检索到相关段落。）",
            request.question
        )
    } else {
        format!(
            "以下是来自知识库的相关段落（用 [n] 编号）：\n\n{context_block}\n\n\
             用户问题：{question}\n\n\
             请只基于上述段落作答，并用地 [n] 标注每条事实的来源；若段落中无答案，明确说「知识库中未找到相关信息」。",
            question = request.question
        )
    };

    // 5) 流式补全（带 run_id 时逐 token 推送；否则阻塞一次）。
    let completion = CompletionRequest::chat(system, user);
    let completion_result = if run_id.is_empty() {
        provider.complete(&completion)
    } else {
        let stream_run_id = run_id.clone();
        provider.complete_streaming(&completion, &|delta| {
            notify(AppEvent::TokenDelta {
                run_id: stream_run_id.clone(),
                delta: delta.to_string(),
            });
        })
    };

    let response = match completion_result {
        Ok(response) => response,
        Err(err) => {
            let _ = telemetry_repository::finish_agent_run(
                conn,
                &agent_run_id,
                "failed",
                0,
                &json!({}),
                Some(&err.to_string()),
                started.elapsed().as_millis() as i64,
            );
            if !run_id.is_empty() {
                notify(AppEvent::AgentFinished {
                    run_id: run_id.clone(),
                    status: "failed".into(),
                });
            }
            return Err(err);
        }
    };

    // 6) 来源：只暴露实际进入上下文的条目，编号与 prompt 中的 [n] 对齐。
    let sources: Vec<AskSource> = pack
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| AskSource {
            index: i + 1,
            kind: kind_to_str(item.kind),
            id: item.id.clone(),
            title: item.title.clone().unwrap_or_default(),
            snippet: item.content.clone(),
        })
        .collect();

    let _ = telemetry_repository::finish_agent_run(
        conn,
        &agent_run_id,
        "success",
        0,
        &json!({
            "question": request.question,
            "answer_chars": response.text.chars().count(),
            "sources": sources.len(),
        }),
        None,
        started.elapsed().as_millis() as i64,
    );
    if !run_id.is_empty() {
        notify(AppEvent::AgentFinished {
            run_id,
            status: "success".into(),
        });
    }

    Ok(AskResponse {
        question: request.question,
        answer: response.text,
        enabled: true,
        note: if pack.truncated {
            Some("部分检索结果因超出上下文预算被截断，已优先保留高相关片段。".into())
        } else {
            None
        },
        sources,
        context_stats: Some(pack.stats),
    })
}

/// role=auto 的轻量路由：按问题类型选角色（确定性关键词，不额外调 LLM）。
fn resolve_auto_role(question: &str) -> AgentRole {
    let q = question.to_lowercase();
    if ["研究", "综述", "调研", "比较", "演进", "research"]
        .iter()
        .any(|k| q.contains(k))
    {
        AgentRole::Research
    } else if ["整理", "合并", "重复", "去重", "清理"]
        .iter()
        .any(|k| q.contains(k))
    {
        AgentRole::Curator
    } else {
        AgentRole::Knowledge
    }
}

/// 编译上下文并写入缓存（INV-20/21：key 变化即失效，无需手工清理）。
fn compile_context(
    items: Vec<ContextItem>,
    budget: &Budget,
    conn: &Connection,
    cache_key: &str,
) -> ContextPack {
    let pack = compile(items, budget);
    if let Ok(value) = serde_json::to_value(&pack) {
        let _ = telemetry_repository::cache_put(conn, cache_key, "knowledge", &value);
    }
    pack
}

/// 缓存 key（INV-20）：question + model + registry/schema 版本 + 库内容指纹。
///
/// 库内容指纹取 documents/claims 的最新时间戳与计数——库一变 key 就变，
/// 旧条目按构造失效，无需手工清理（INV-21）。
fn context_cache_key(conn: &Connection, question: &str, model: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let document_fingerprint = conn.query_row(
        "SELECT IFNULL(MAX(updated_at), '') || ':' || COUNT(*) FROM documents",
        [],
        |row| row.get::<_, String>(0),
    );
    // claims 的指纹额外纳入 superseded 计数：状态迁移（取代）不一定改变
    // recorded_at / 行数，但会改变「当前知识」，必须让缓存失效。
    let claim_fingerprint = conn.query_row(
        "SELECT IFNULL(MAX(recorded_at), '') || ':' || COUNT(*)
                || ':' || IFNULL(SUM(CASE WHEN status = 'superseded' THEN 1 ELSE 0 END), 0)
         FROM claims",
        [],
        |row| row.get::<_, String>(0),
    );

    // 指纹查询失败时若退化为空串参与哈希，可能与其它库状态碰撞。
    // 这里改为返回带随机 nonce 的 key，保证本次必然 miss。
    let (document_fingerprint, claim_fingerprint) = match (document_fingerprint, claim_fingerprint) {
        (Ok(documents), Ok(claims)) => (documents, claims),
        _ => {
            crate::log_warn!("上下文缓存指纹查询失败，本次跳过缓存");
            let mut hasher = DefaultHasher::new();
            question.hash(&mut hasher);
            uuid::Uuid::new_v4().hash(&mut hasher);
            return format!("ask:miss:{:016x}", hasher.finish());
        }
    };

    let mut hasher = DefaultHasher::new();
    question.hash(&mut hasher);
    model.hash(&mut hasher);
    registry::version().hash(&mut hasher);
    db::SCHEMA_VERSION.hash(&mut hasher);
    document_fingerprint.hash(&mut hasher);
    claim_fingerprint.hash(&mut hasher);
    format!("ask:{:016x}", hasher.finish())
}

/// LoadStrategy → context_sections.policy（CHECK 约束的大写四值）。
fn policy_str(strategy: LoadStrategy) -> &'static str {
    match strategy {
        LoadStrategy::Load => "LOAD",
        LoadStrategy::Summarize => "SUMMARIZE",
        LoadStrategy::RetrieveLater => "RETRIEVE_LATER",
        LoadStrategy::NeverLoad => "NEVER_LOAD",
    }
}

fn kind_to_str(kind: ContextKind) -> String {
    match kind {
        ContextKind::Document => "document",
        ContextKind::Chunk => "chunk",
        ContextKind::Claim => "claim",
        ContextKind::Entity => "entity",
        ContextKind::Evidence => "evidence",
    }
    .to_string()
}
