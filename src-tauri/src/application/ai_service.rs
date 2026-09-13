//! AI 抽取编排（Phase 5）。
//!
//! 职责：把一篇文档交给 Provider 抽取结构化的 Claim 候选，再用受控词表
//! 校验谓语合法性，**只预览、不落库**——落库走已有的 `create_claim` +
//! `analyze_document`，由用户在 Review 中最终决定（PRD「AI suggests, user decides」）。
//!
//! 不写业务逻辑以外的东西：状态、校验、事务都在它该在的地方。

use serde::Deserialize;
use rusqlite::Connection;

use crate::ai::config::AiConfig;
use crate::ai::provider::{default_provider, CompletionRequest};
use crate::application::dto::{ExtractedClaim, ExtractionReport};
use crate::domain::common::ids::DocumentId;
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::domain::ontology::registry;
use crate::error::{AppError, AppResult};
use crate::infrastructure::document_repository;

/// 从一篇文档抽取 Claim 候选（预览，不落库）。
pub fn extract_claims(conn: &Connection, document_id: &str) -> AppResult<ExtractionReport> {
    let doc_id = DocumentId::from_raw(document_id.trim());
    let document = document_repository::find_by_id(conn, &doc_id)?
        .ok_or_else(|| AppError::NotFound(format!("文档 {document_id} 不存在")))?;
    let chunks = document_repository::list_chunks(conn, &doc_id)?;

    let provider = default_provider(conn);
    if !provider.enabled() {
        return Ok(ExtractionReport {
            document_id: document_id.to_string(),
            provider: provider.name().to_string(),
            enabled: false,
            note: Some(
                "AI 未启用：未配置 WIKIYA_API_KEY（可选 WIKIYA_BASE_URL / WIKIYA_MODEL）。\
                 配置后重启应用即可启用抽取。"
                    .into(),
            ),
            extracted: Vec::new(),
        });
    }

    // 把切片拼成带下标的语料，便于模型回指 sourceChunk。
    let corpus = chunks
        .iter()
        .map(|chunk| format!("[{}] {}", chunk.chunk_index, chunk.content))
        .collect::<Vec<_>>()
        .join("\n");

    let predicates = registry::registry()
        .claim_predicates
        .iter()
        .map(|spec| spec.as_str().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let system = format!(
        "你是一个严谨的知识抽取器。从给定文本中抽取结构化的 Claim（断言）。\n\
         允许使用的 predicate（谓语）只能是以下受控词表之一：{predicates}。\n\
         对每条 Claim，输出字段：subject（主语实体名）、predicate（必须∈上述词表）、\
         objectText（宾语，可为空）、content（完整陈述句，可为空）、claimType、\
         polarity（positive/negative）、modality、confidence（0..1）、sourceChunk（切片下标）、\
         sentence（原文句子）。\n\
         只输出一个 JSON 对象，形如 {{\"claims\":[ ... ]}}，不要任何解释或 Markdown。\
         若文本无可抽取的 Claim，输出 {{\"claims\":[]}}。"
    );
    let user = format!("文档标题：{}\n\n正文切片：\n{}", document.title, corpus);

    let request = CompletionRequest::structured(system, user);
    crate::log_info!(
        "开始抽取：文档 `{}`（{document_id}），切片 {} 个，provider={}，model={}",
        document.title,
        chunks.len(),
        provider.name(),
        AiConfig::from_settings(conn).model
    );
    let response = provider.complete(&request)?;

    // 宽松解析：容忍模型偶尔加上的 Markdown 围栏或前后解释文字，
    // 同时兼容「对象包裹 {\"claims\":[...]}」与「裸数组 [...]」两种返回形态。
    let raw: Vec<RawClaim> = parse_claims(&response.text).map_err(|err| {
        crate::log_error!(
            "模型返回内容无法解析为 Claim JSON：{err}；原文（前 800 字符）：{}",
            crate::logging::clip(&response.text, 800)
        );
        AppError::Internal(format!("AI 返回的 JSON 解析失败：{err}"))
    })?;
    crate::log_info!("抽取完成：解析出 {} 条候选 Claim", raw.len());

    let mut extracted = Vec::with_capacity(raw.len());
    for item in raw {
        // 谓语合法性由受控词表把关：不合法的直接标为 rejected，绝不悄悄落库。
        let predicate_canon = ClaimPredicate::canonical(&item.predicate);
        let (predicate, reject_reason) = match predicate_canon {
            Ok(parsed) => (parsed.as_str().to_string(), None),
            Err(_) => (
                item.predicate.clone(),
                Some(format!("谓语 `{}` 不在受控词表中，已排除", item.predicate)),
            ),
        };

        extracted.push(ExtractedClaim {
            subject: item.subject,
            predicate,
            object_text: item.object_text,
            content: item.content,
            claim_type: item.claim_type,
            polarity: item.polarity,
            modality: item.modality,
            confidence: item.confidence,
            source_chunk_index: item.source_chunk,
            source_quote: item.sentence.clone(),
            sentence: item.sentence,
            accepted: reject_reason.is_none(),
            reject_reason,
        });
    }

    Ok(ExtractionReport {
        document_id: document_id.to_string(),
        provider: provider.name().to_string(),
        enabled: true,
        note: None,
        extracted,
    })
}

/// Provider 原始返回的 Claim（字段宽松，校验前先用它接住）。
#[derive(Debug, Deserialize)]
struct RawClaim {
    subject: String,
    predicate: String,
    #[serde(default)]
    object_text: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    claim_type: Option<String>,
    #[serde(default)]
    polarity: Option<String>,
    #[serde(default)]
    modality: Option<String>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    source_chunk: Option<usize>,
    #[serde(default)]
    sentence: Option<String>,
}

/// 容错解析模型返回的 Claim JSON。
///
/// 兼容两种形态：对象包裹 `{"claims":[...]}`（与 `json_object` 模式最契合）
/// 与裸数组 `[...]`。同时容忍模型偶尔夹带的 Markdown 围栏或前后解释文字：
/// 优先在文本中定位首个 `[`/`]` 或 `{`/`}` 包裹的 JSON 片段再解析。
fn parse_claims(text: &str) -> Result<Vec<RawClaim>, serde_json::Error> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        // 不应到达这里（provider 已拦截空响应），但防御性处理。
        return serde_json::from_str::<Vec<RawClaim>>("").map(|_| Vec::new());
    }

    let value: serde_json::Value = {
        // 先尝试整段解析；失败再退回切片提取，兼容围栏/杂散文字。
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => v,
            Err(_) => {
                let (start, end) = if let (Some(s), Some(e)) = (trimmed.find('['), trimmed.rfind(']'))
                {
                    (s, e)
                } else if let (Some(s), Some(e)) =
                    (trimmed.find('{'), trimmed.rfind('}'))
                {
                    (s, e)
                } else {
                    return Err(serde_json::from_str::<serde_json::Value>(trimmed).unwrap_err());
                };
                serde_json::from_str(&trimmed[start..=end])?
            }
        }
    };

    let arr = match &value {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(map) => map
            .get("claims")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    serde_json::from_value(serde_json::Value::Array(arr))
}
