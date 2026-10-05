//! Extraction Run 编排（EXTRACTION-001）。
//!
//! 把"分析文档"从一次阻塞式同步调用，改造成 **Run 化的后台任务**：
//!
//! - `create_run` 立刻落库并返回 `run_id`（命令秒回，前端拿 id 去订阅）；
//! - `execute` 在后台跑完整管线（Preparing → Chunking → Extracting →
//!   Validating → Comparing → Finalizing），每推进一步就持久化 + 发事件；
//! - 任务状态全部落在 `extraction_runs` 表，页面关了 / 应用关了都还在；
//! - 重启时 `recover_interrupted_runs` 把"上次还在跑"的 Run 标记为
//!   `interrupted`，不假装还在运行。
//!
//! 诚实边界与 `ai_service` 一致：候选**只预览、不落库**，最终由用户在
//! Review 里决定（PRD「AI suggests, user decides」）。

use std::collections::HashSet;
use std::path::PathBuf;

use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use crate::ai::accounting::TokenUsage;
use crate::ai::config::AiConfig;
use crate::ai::provider::default_provider;
use crate::application::ai_service;
use crate::application::dto::{ExtractedClaim, ExtractionRunDto, ExtractionRunSummary};
use crate::domain::common::ids::DocumentId;
use crate::domain::extraction::{ExtractionRun, ExtractionRunStatus, ExtractionStage};
use crate::domain::knowledge::candidate::{
    classify_support, Candidate, CandidateStatus, SupportLevel,
};
use crate::domain::knowledge::claim::ClaimObject;
use crate::domain::run::RunType;
use crate::error::{AppError, AppResult};
use crate::events::{RunEvent, RunSink};
use crate::infrastructure::{
    candidate_repository, claim_repository, db, document_repository, extraction_run_repository,
    run_repository,
};

/// 创建一条 Run（初始 `queued` / `preparing`），立即返回 id。
pub fn create_run(conn: &Connection, document_id: &str) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let started_at = db::now(conn)?;
    let run = ExtractionRun {
        id: id.clone(),
        document_id: document_id.to_string(),
        status: ExtractionRunStatus::Queued,
        stage: ExtractionStage::Preparing,
        total_chunks: 0,
        processed_chunks: 0,
        candidates_found: 0,
        changes_found: 0,
        result_json: None,
        started_at,
        finished_at: None,
        error_code: None,
        error_message: None,
    };
    extraction_run_repository::create(conn, &run)?;
    // 统一登记（M1）：同一 id 在 runs 表有一行，Trace 才能跨类型串联。
    let metadata = serde_json::json!({ "document_id": document_id }).to_string();
    run_repository::register(
        conn,
        &id,
        RunType::Extraction,
        "ExtractionAgent",
        None,
        &metadata,
    )?;
    Ok(id)
}

/// 读取一条 Run 的快照。
pub fn get_run(conn: &Connection, id: &str) -> AppResult<ExtractionRunDto> {
    let run = extraction_run_repository::get(conn, id)?;
    to_dto(&run, &AiConfig::from_settings(conn))
}

/// 最近的 Run（新→旧）。
pub fn list_runs(conn: &Connection, limit: usize) -> AppResult<Vec<ExtractionRunDto>> {
    let runs = extraction_run_repository::list_recent(conn, limit)?;
    // PERF-09：AI 配置只解析**一次**。`AiConfig::from_settings` 每次都要读
    // 5~6 条 settings，其中 API Key 还要做一次 AES-256-GCM **解密**；逐行
    // 解析等于把这份开销乘以行数（Activity 每 400ms 刷 12 行、文档页挂载
    // 时拉 50 行）。配置在整个查询期间不会变，读一次就够。
    let config = AiConfig::from_settings(conn);
    runs.iter()
        .map(|run| {
            let mut dto = to_dto(run, &config)?;
            // 列表不需要 `result_json`（它是含全部抽取 Claim 的完整报告）：
            // 列表只用于 Activity/首页概览，逐条带上会成倍放大 IPC 体积
            // （Activity 每次 Run 事件都会整表刷新）。需要详情时走
            // `get_extraction_run`，那里仍然返回完整结果。
            dto.result_json = None;
            Ok(dto)
        })
        .collect()
}

/// 取消一条还在跑的 Run。已终态则返回 `false`。
///
/// 这里只把状态改成 `cancelled`；后台任务在"批与批之间"检查到后礼貌停下
/// （先完成当前请求，不强杀线程）。
pub fn cancel_run(conn: &Connection, id: &str) -> AppResult<bool> {
    let run = extraction_run_repository::get(conn, id)?;
    if run.status.is_terminal() {
        return Ok(false);
    }
    extraction_run_repository::finish(
        conn,
        id,
        ExtractionRunStatus::Cancelled,
        run.result_json.as_deref(),
        None,
        None,
    )?;
    run_repository::finish(conn, id, ExtractionRunStatus::Cancelled, None, None)?;
    Ok(true)
}

/// 启动时把"上次还在跑"的 Run 标记为 interrupted（不假装还在运行）。
pub fn recover_interrupted_runs(conn: &Connection) -> AppResult<usize> {
    let n = extraction_run_repository::mark_running_as_interrupted(conn)?;
    run_repository::mark_stale_interrupted(conn)?;
    Ok(n)
}

/// 启动后台执行：命令拿到 `run_id` 后即可返回，真正的活在这里跑。
///
/// 内部用 `spawn_blocking` 承载 SQLite 与 `reqwest::blocking` 的同步调用，
/// 避免阻塞 Tokio 的异步 worker（见架构文档 §7）。
pub async fn execute(app: AppHandle, db_path: PathBuf, run_id: String) {
    // 事件经 Tauri 全局频道 `extraction-events` 推前端。sink 持有 AppHandle 克隆，
    // 与 commands 层解耦（Service 不依赖 Commands）。
    // 统一 Run 事件（M1）：单一 run-events 频道，过渡期与旧频道并存。
    let run_emitter = app.clone();
    let run_events: RunSink = std::sync::Arc::new(move |event: &RunEvent| {
        let _ = run_emitter.emit("run-events", event);
    });

    let db_path_for_task = db_path.clone();
    let run_id_for_task = run_id.clone();
    let join = tauri::async_runtime::spawn_blocking(move || {
        run_pipeline(&db_path_for_task, &run_id_for_task, &run_events)
    })
    .await;

    match join {
        Ok(Ok(())) => {}
        Ok(Err(err)) => mark_failed(&db_path, &run_id, &err),
        Err(err) => mark_failed(
            &db_path,
            &run_id,
            &AppError::Internal(format!("抽取任务线程异常终止：{err}")),
        ),
    }
}

fn mark_failed(db_path: &PathBuf, run_id: &str, err: &AppError) {
    if let Ok(conn) = db::open(db_path) {
        let message = err.to_string();
        let _ = extraction_run_repository::finish(
            &conn,
            run_id,
            ExtractionRunStatus::Failed,
            None,
            Some("INTERNAL_ERROR"),
            Some(&message),
        );
        // 统一登记处同步收口（M1）。
        let _ = run_repository::finish(
            &conn,
            run_id,
            ExtractionRunStatus::Failed,
            Some("INTERNAL_ERROR"),
            Some(&message),
        );
    }
}

pub(crate) fn to_dto(run: &ExtractionRun, config: &AiConfig) -> AppResult<ExtractionRunDto> {
    // PR-04：从序列化的 ExtractionReport 里取出真实 token 账本，并据当前
    // AI 配置估算成本（未知模型返回 null，不编造价格）。
    let usage = run
        .result_json
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get("usage").cloned())
        .and_then(|u| serde_json::from_value::<TokenUsage>(u).ok());
    let cost_usd = usage.and_then(|u| u.estimate_cost_usd(&config.model, &config.embedding_model));
    Ok(ExtractionRunDto {
        id: run.id.clone(),
        document_id: run.document_id.clone(),
        status: run.status.as_str().to_string(),
        stage: run.stage.as_str().to_string(),
        total_chunks: run.total_chunks,
        processed_chunks: run.processed_chunks,
        candidates_found: run.candidates_found,
        changes_found: run.changes_found,
        result_json: run.result_json.clone(),
        started_at: run.started_at.clone(),
        finished_at: run.finished_at.clone(),
        error_code: run.error_code.clone(),
        error_message: run.error_message.clone(),
        usage,
        cost_usd,
    })
}

/// 后台管线：状态 / 阶段 / 进度逐段持久化并推送事件（旧频道 + 统一 RunEvent 双发）。
fn run_pipeline(db_path: &PathBuf, run_id: &str, run_sink: &RunSink) -> AppResult<()> {
    let conn = db::open(db_path)?;
    let run = extraction_run_repository::get(&conn, run_id)?;
    let document_id = run.document_id.clone();

    // ---- Preparing ----
    extraction_run_repository::set_status(&conn, run_id, ExtractionRunStatus::Running)?;
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Preparing)?;
    run_repository::set_status(&conn, run_id, ExtractionRunStatus::Running)?;
    run_repository::set_stage(&conn, run_id, ExtractionStage::Preparing.as_str())?;
    run_sink(&RunEvent::Started {
        run_id: run_id.to_string(),
        run_type: RunType::Extraction,
    });

    let doc_id = DocumentId::from_raw(&document_id);
    let document = document_repository::find_by_id(&conn, &doc_id)?
        .ok_or_else(|| AppError::NotFound(format!("文档 {document_id} 不存在")))?;
    let title = document.title.clone();

    // ---- Chunking ----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Chunking)?;
    run_repository::set_stage(&conn, run_id, ExtractionStage::Chunking.as_str())?;
    let chunks = document_repository::list_chunks(&conn, &doc_id)?;
    let total = chunks.len();
    extraction_run_repository::set_progress(&conn, run_id, 0, total as i64)?;
    run_sink(&RunEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Chunking.as_str().to_string(),
    });

    // 动手前确认 AI 可用：不可用则诚实结束（结果标记为未启用），不报错。
    let provider = default_provider(&conn);
    if !provider.enabled() {
        let report = ExtractionRunSummary {
            document_id: document_id.clone(),
            provider: provider.name().to_string(),
            enabled: false,
            note: Some(
                "AI 未启用：未配置 WIKIYA_API_KEY（可选 WIKIYA_BASE_URL / WIKIYA_MODEL）。\
                 配置后重启应用即可启用抽取。"
                    .into(),
            ),
            candidate_count: 0,
            directly: 0,
            partially: 0,
            unsupported: 0,
            duplicates: 0,
            changes_estimate: 0,
            usage: TokenUsage::default(),
        };
        let result_json = serde_json::to_string(&report).ok();
        extraction_run_repository::finish(
            &conn,
            run_id,
            ExtractionRunStatus::Completed,
            result_json.as_deref(),
            None,
            None,
        )?;
        return Ok(());
    }

    // ---- Extracting（分批）----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Extracting)?;
    run_repository::set_stage(&conn, run_id, ExtractionStage::Extracting.as_str())?;
    run_sink(&RunEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Extracting.as_str().to_string(),
    });
    let system = ai_service::extraction_system_prompt();
    let mut processed = 0usize;
    // 真实 token 账本：跨批次累计（PR-04）。
    let mut all_usage = TokenUsage::default();
    // PERF-05：不再把全部候选 accumulate 到 Finalizing（那是纯内存开销——
    // 明细已逐批落库，`result_json` 也只存摘要）。此处只保留统计量。
    let mut candidate_count = 0i64;
    let mut accepted_count = 0i64;
    let mut support_counts = SupportCounts::default();
    // 重复候选跨批次增量统计：签名集只建一次，不必留全部候选在内存里。
    let mut dup_counter = DuplicateCounter::new(&conn, &document_id);
    // 分批送模型（ai_service::batch_chunks：块数 + 字符双预算），
    // 控制单次输出体量，避免被 max_tokens 截断导致 JSON 解析失败。
    for batch in ai_service::batch_chunks(&chunks) {
        // 批与批之间检查取消：用户点了取消就礼貌停下（先完成当前请求）。
        let current = extraction_run_repository::get(&conn, run_id)?;
        if current.status == ExtractionRunStatus::Cancelled {
            run_sink(&RunEvent::Cancelled {
                run_id: run_id.to_string(),
            });
            return Ok(());
        }

        let corpus = batch
            .iter()
            .map(|(idx, content)| format!("[{idx}] {content}"))
            .collect::<Vec<_>>()
            .join("\n");
        let user = format!("文档标题：{title}\n\n正文切片：\n{corpus}");
        let (batch_claims, batch_usage) = ai_service::extract_corpus(&conn, system.clone(), user)?;
        all_usage.add(&batch_usage);
        // M6 修正（外部复盘准确指出）：候选**逐批立即持久化**——
        // Batch 4 失败时 Batch 1~3 的候选已经在库，不再等 Validating 统一写入。
        for claim in &batch_claims {
            // 本地 grounding：取该候选来源切片（应在本批内）的原文，
            // 校验 quote 是否落在其中——无需回调 LLM。
            let chunk_text = claim
                .source_chunk_index
                .and_then(|idx| {
                    batch
                        .iter()
                        .find(|(i, _)| *i as i64 == idx as i64)
                        .map(|(_, c)| c.as_str())
                });
            let support_level = classify_support(&claim.source_quote, chunk_text);
            support_counts.observe(support_level);
            let candidate = Candidate {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                document_id: document_id.clone(),
                subject: claim.subject.clone(),
                predicate: claim.predicate.clone(),
                object_text: claim.object_text.clone(),
                content: claim.content.clone(),
                claim_type: claim.claim_type.clone(),
                polarity: claim.polarity.clone(),
                modality: claim.modality.clone(),
                confidence: claim.confidence,
                source_chunk_index: claim.source_chunk_index.map(|v| v as i64),
                source_quote: claim.source_quote.clone(),
                sentence: claim.sentence.clone(),
                support_level,
                status: CandidateStatus::Pending,
                accepted_claim_id: None,
                reject_reason: claim.reject_reason.clone(),
                created_at: String::new(),
            };
            candidate_repository::insert(&conn, &candidate)?;
            candidate_count += 1;
        }
        // PERF-05：重复候选增量统计（不必留全部候选在内存里）。
        let accepted_in_batch: Vec<&ExtractedClaim> =
            batch_claims.iter().filter(|c| c.accepted).collect();
        accepted_count += accepted_in_batch.len() as i64;
        dup_counter.observe(&accepted_in_batch);
        let batch_count = batch_claims.len();
        processed += batch.len();
        extraction_run_repository::set_progress(&conn, run_id, processed as i64, total as i64)?;
        run_sink(&RunEvent::Progress {
            run_id: run_id.to_string(),
            processed,
            total,
        });
        run_sink(&RunEvent::CandidateCreated {
            run_id: run_id.to_string(),
            count: batch_count,
        });
    }

    // ---- Validating（候选已在 Extracting 阶段逐批落库——M6 修正）----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Validating)?;
    run_repository::set_stage(&conn, run_id, ExtractionStage::Validating.as_str())?;
    run_sink(&RunEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Validating.as_str().to_string(),
    });
    let candidates_found = candidate_count;

    // ---- Comparing（预估变更数：签名去重，仅供排序参考；真正的演化
    // 分类在用户 accept 候选时由 analyze_document 产生并进 Review）----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Comparing)?;
    run_repository::set_stage(&conn, run_id, ExtractionStage::Comparing.as_str())?;
    run_sink(&RunEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Comparing.as_str().to_string(),
    });
    let duplicates = dup_counter.duplicates();
    let changes = accepted_count.saturating_sub(duplicates);
    extraction_run_repository::set_counts(&conn, run_id, candidates_found, changes)?;

    // ---- Finalizing ----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Finalizing)?;
    run_repository::set_stage(&conn, run_id, ExtractionStage::Finalizing.as_str())?;
    run_sink(&RunEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Finalizing.as_str().to_string(),
    });
    let report = ExtractionRunSummary {
        document_id: document_id.clone(),
        provider: provider.name().to_string(),
        enabled: true,
        note: None,
        candidate_count,
        directly: support_counts.directly,
        partially: support_counts.partially,
        unsupported: support_counts.unsupported,
        duplicates,
        changes_estimate: changes,
        usage: all_usage,
    };
    let result_json = serde_json::to_string(&report).ok();
    extraction_run_repository::finish(
        &conn,
        run_id,
        ExtractionRunStatus::Completed,
        result_json.as_deref(),
        None,
        None,
    )?;
    // PR-07：把真实 token 账本落到统一登记处，使 Run Trace 能显示成本。
    run_repository::set_usage(&conn, run_id, &all_usage)?;
    run_repository::finish(&conn, run_id, ExtractionRunStatus::Completed, None, None)?;
    run_sink(&RunEvent::Completed {
        run_id: run_id.to_string(),
    });
    Ok(())
}

/// 支持度分档计数（PERF-05：写进 Run 摘要，供 UI 如实展示锚定质量）。
#[derive(Default)]
struct SupportCounts {
    directly: i64,
    partially: i64,
    unsupported: i64,
}

impl SupportCounts {
    fn observe(&mut self, level: SupportLevel) {
        match level {
            SupportLevel::Directly => self.directly += 1,
            SupportLevel::Partially => self.partially += 1,
            SupportLevel::Unsupported => self.unsupported += 1,
        }
    }
}

/// 跨批次增量统计重复候选（PERF-05）。
///
/// 旧实现为了算这一个数而把全部候选 accumulate 到 Finalizing。改为：库内已有
/// Claim 的签名集**只建一次**，之后逐批喂入并累加，内存里不留候选。
///
/// 语义保持与旧实现**完全一致**（已知局限：同文档内互相重复的候选不计入，
/// 因为候选签名不回插集合）——性能 PR 不应顺手改变上报的数字。
struct DuplicateCounter {
    signatures: HashSet<(String, String, String)>,
    duplicates: usize,
}

impl DuplicateCounter {
    /// 建一次库内已有 Claim 的签名集；读取失败时退化为空集（重复数记 0）。
    fn new(conn: &Connection, document_id: &str) -> Self {
        let mut signatures: HashSet<(String, String, String)> = HashSet::new();
        if let Ok(rows) = claim_repository::list_by_document(conn, &DocumentId::from_raw(document_id))
        {
            for row in &rows {
                signatures.insert((
                    row.subject_name.to_lowercase(),
                    row.claim.predicate.to_string(),
                    object_text_of(&row.claim.object).to_lowercase(),
                ));
            }
        }
        DuplicateCounter {
            signatures,
            duplicates: 0,
        }
    }

    /// 喂入一批**已通过受控词表校验**的候选，累加命中已有签名的数量。
    fn observe(&mut self, accepted: &[&ExtractedClaim]) {
        for candidate in accepted {
            let object = candidate
                .object_text
                .clone()
                .unwrap_or_default()
                .to_lowercase();
            let signature = (
                candidate.subject.to_lowercase(),
                candidate.predicate.clone(),
                object,
            );
            if self.signatures.contains(&signature) {
                self.duplicates += 1;
            }
        }
    }

    fn duplicates(&self) -> i64 {
        self.duplicates as i64
    }
}

/// 从 Claim 的宾语里取出可读文本（用于去重签名）。
fn object_text_of(object: &Option<ClaimObject>) -> String {
    match object {
        Some(ClaimObject::Literal(text)) => text.clone(),
        Some(ClaimObject::Number(value)) => value.to_string(),
        Some(ClaimObject::Boolean(value)) => value.to_string(),
        Some(ClaimObject::Date(value)) => value.clone(),
        Some(ClaimObject::Entity(_)) | None => String::new(),
    }
}
