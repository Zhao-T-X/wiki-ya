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

use crate::ai::provider::default_provider;
use crate::application::ai_service;
use crate::application::dto::{ExtractionReport, ExtractionRunDto, ExtractedClaim};
use crate::domain::common::ids::DocumentId;
use crate::domain::extraction::{ExtractionRun, ExtractionRunStatus, ExtractionStage};
use crate::domain::knowledge::claim::ClaimObject;
use crate::error::{AppError, AppResult};
use crate::events::{ExtractionEvent, ExtractionSink};
use crate::infrastructure::{claim_repository, db, document_repository, extraction_run_repository};

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
    Ok(id)
}

/// 读取一条 Run 的快照。
pub fn get_run(conn: &Connection, id: &str) -> AppResult<ExtractionRunDto> {
    let run = extraction_run_repository::get(conn, id)?;
    Ok(to_dto(&run))
}

/// 最近的 Run（新→旧）。
pub fn list_runs(conn: &Connection, limit: usize) -> AppResult<Vec<ExtractionRunDto>> {
    let runs = extraction_run_repository::list_recent(conn, limit)?;
    Ok(runs.iter().map(to_dto).collect())
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
    Ok(true)
}

/// 启动时把"上次还在跑"的 Run 标记为 interrupted（不假装还在运行）。
pub fn recover_interrupted_runs(conn: &Connection) -> AppResult<usize> {
    extraction_run_repository::mark_running_as_interrupted(conn)
}

/// 启动后台执行：命令拿到 `run_id` 后即可返回，真正的活在这里跑。
///
/// 内部用 `spawn_blocking` 承载 SQLite 与 `reqwest::blocking` 的同步调用，
/// 避免阻塞 Tokio 的异步 worker（见架构文档 §7）。
pub async fn execute(app: AppHandle, db_path: PathBuf, run_id: String) {
    // 事件经 Tauri 全局频道 `extraction-events` 推前端。sink 持有 AppHandle 克隆，
    // 与 commands 层解耦（Service 不依赖 Commands）。
    let emitter = app.clone();
    let sink: ExtractionSink = std::sync::Arc::new(move |event: &ExtractionEvent| {
        let _ = emitter.emit("extraction-events", event);
    });

    let db_path_for_task = db_path.clone();
    let run_id_for_task = run_id.clone();
    let join = tauri::async_runtime::spawn_blocking(move || {
        run_pipeline(&db_path_for_task, &run_id_for_task, &sink)
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
    }
}

fn to_dto(run: &ExtractionRun) -> ExtractionRunDto {
    ExtractionRunDto {
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
    }
}

/// 后台管线：状态 / 阶段 / 进度逐段持久化并推送事件。
fn run_pipeline(db_path: &PathBuf, run_id: &str, sink: &ExtractionSink) -> AppResult<()> {
    let conn = db::open(db_path)?;
    let run = extraction_run_repository::get(&conn, run_id)?;
    let document_id = run.document_id.clone();

    // ---- Preparing ----
    extraction_run_repository::set_status(&conn, run_id, ExtractionRunStatus::Running)?;
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Preparing)?;
    sink(&ExtractionEvent::Started {
        run_id: run_id.to_string(),
        document_id: document_id.clone(),
    });

    let doc_id = DocumentId::from_raw(&document_id);
    let document = document_repository::find_by_id(&conn, &doc_id)?
        .ok_or_else(|| AppError::NotFound(format!("文档 {document_id} 不存在")))?;
    let title = document.title.clone();

    // ---- Chunking ----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Chunking)?;
    let chunks = document_repository::list_chunks(&conn, &doc_id)?;
    let total = chunks.len();
    extraction_run_repository::set_progress(&conn, run_id, 0, total as i64)?;
    sink(&ExtractionEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Chunking,
    });

    // 动手前确认 AI 可用：不可用则诚实结束（结果标记为未启用），不报错。
    let provider = default_provider(&conn);
    if !provider.enabled() {
        let report = ExtractionReport {
            document_id: document_id.clone(),
            provider: provider.name().to_string(),
            enabled: false,
            note: Some(
                "AI 未启用：未配置 WIKIYA_API_KEY（可选 WIKIYA_BASE_URL / WIKIYA_MODEL）。\
                 配置后重启应用即可启用抽取。"
                    .into(),
            ),
            extracted: Vec::new(),
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
        sink(&ExtractionEvent::Completed {
            run_id: run_id.to_string(),
        });
        return Ok(());
    }

    // ---- Extracting（分批）----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Extracting)?;
    sink(&ExtractionEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Extracting,
    });
    let system = ai_service::extraction_system_prompt();
    let mut all: Vec<ExtractedClaim> = Vec::new();
    let mut processed = 0usize;
    // 分批送模型（ai_service::batch_chunks：块数 + 字符双预算），
    // 控制单次输出体量，避免被 max_tokens 截断导致 JSON 解析失败。
    for batch in ai_service::batch_chunks(&chunks) {
        // 批与批之间检查取消：用户点了取消就礼貌停下（先完成当前请求）。
        let current = extraction_run_repository::get(&conn, run_id)?;
        if current.status == ExtractionRunStatus::Cancelled {
            sink(&ExtractionEvent::Cancelled {
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
        let batch_claims = ai_service::extract_corpus(&conn, system.clone(), user)?;
        all.extend(batch_claims);
        processed += batch.len();
        extraction_run_repository::set_progress(&conn, run_id, processed as i64, total as i64)?;
        sink(&ExtractionEvent::Progress {
            run_id: run_id.to_string(),
            processed,
            total,
        });
    }
    sink(&ExtractionEvent::CandidateFound {
        run_id: run_id.to_string(),
        count: all.len(),
    });

    // ---- Validating ----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Validating)?;
    sink(&ExtractionEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Validating,
    });
    let accepted: Vec<&ExtractedClaim> = all.iter().filter(|c| c.accepted).collect();
    let candidates_found = all.len() as i64;

    // ---- Comparing（相对库内已有知识去重，估算"变更数"）----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Comparing)?;
    sink(&ExtractionEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Comparing,
    });
    let duplicates = count_duplicates(&conn, &document_id, &accepted);
    let changes = (accepted.len().saturating_sub(duplicates)) as i64;
    extraction_run_repository::set_counts(&conn, run_id, candidates_found, changes)?;
    sink(&ExtractionEvent::ComparisonCompleted {
        run_id: run_id.to_string(),
        changes: changes as usize,
    });

    // ---- Finalizing ----
    extraction_run_repository::set_stage(&conn, run_id, ExtractionStage::Finalizing)?;
    sink(&ExtractionEvent::StageChanged {
        run_id: run_id.to_string(),
        stage: ExtractionStage::Finalizing,
    });
    let report = ExtractionReport {
        document_id: document_id.clone(),
        provider: provider.name().to_string(),
        enabled: true,
        note: None,
        extracted: all,
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
    sink(&ExtractionEvent::Completed {
        run_id: run_id.to_string(),
    });
    Ok(())
}

/// 估算"相对库内已有知识的新增变更数"：把已接受的候选与同文档现有 Claim 做签名比对。
fn count_duplicates(
    conn: &Connection,
    document_id: &str,
    accepted: &[&ExtractedClaim],
) -> usize {
    let existing = match claim_repository::list_by_document(conn, &DocumentId::from_raw(document_id))
    {
        Ok(rows) => rows,
        Err(_) => return 0,
    };
    let mut signatures: HashSet<(String, String, String)> = HashSet::new();
    for row in &existing {
        let object = object_text_of(&row.claim.object).to_lowercase();
        signatures.insert((
            row.subject_name.to_lowercase(),
            row.claim.predicate.to_string(),
            object,
        ));
    }
    let mut duplicates = 0usize;
    for candidate in accepted {
        let object = candidate
            .object_text
            .clone()
            .unwrap_or_default()
            .to_lowercase();
        let signature = (candidate.subject.to_lowercase(), candidate.predicate.clone(), object);
        if signatures.contains(&signature) {
            duplicates += 1;
        }
    }
    duplicates
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
