//! AI 抽取编排（Phase 5）。
//!
//! 职责：把一篇文档交给 Provider 抽取结构化的 Claim 候选，再用受控词表
//! 校验谓语合法性，**只预览、不落库**——落库走已有的 `create_claim` +
//! `analyze_document`，由用户在 Review 中最终决定（PRD「AI suggests, user decides」）。
//!
//! 不写业务逻辑以外的东西：状态、校验、事务都在它该在的地方。

use serde::Deserialize;
use rusqlite::Connection;

use crate::ai::provider::{default_provider, CompletionRequest};
use crate::application::dto::{ExtractedClaim, ExtractionReport};
use crate::domain::common::ids::DocumentId;
use crate::domain::knowledge::chunk::Chunk;
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

    // 分批送模型：整篇语料一次送出会让输出体量超出 max_tokens 而被截断，
    // 产生「EOF while parsing a string」这类解析失败（EXTRACTION-001 复盘）。
    let system = extraction_system_prompt();
    let mut extracted = Vec::new();
    for batch in batch_chunks(&chunks) {
        let corpus = batch
            .iter()
            .map(|(idx, content)| format!("[{idx}] {content}"))
            .collect::<Vec<_>>()
            .join("\n");
        let user = format!("文档标题：{}\n\n正文切片：\n{}", document.title, corpus);
        extracted.extend(extract_corpus(conn, system.clone(), user)?);
    }

    Ok(ExtractionReport {
        document_id: document_id.to_string(),
        provider: provider.name().to_string(),
        enabled: true,
        note: None,
        extracted,
    })
}

/// 构造抽取用的系统提示词（受控词表 + 离散字段约束）。
///
/// 被 [`extract_claims`] 与后台 `extraction_service` 共用，避免两处漂移。
pub fn extraction_system_prompt() -> String {
    let predicates = registry::registry()
        .claim_predicates
        .iter()
        .map(|spec| spec.as_str().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "你是一个严谨的知识抽取器。从给定文本中抽取结构化的 Claim（断言）。\n\
         允许使用的 predicate（谓语）只能是以下受控词表之一：{predicates}。\n\
         对每条 Claim，输出字段：subject（主语实体名）、predicate（必须∈上述词表）、\
         objectText（宾语，可为空）、content（完整陈述句，可为空）、claimType、\
         polarity、modality、confidence（0..1）、sourceChunk（切片下标）、\
         sentence（原文句子）。\n\
         其中离散字段只能取下列受控取值（小写，禁止用其他词，也不要用中文或句子）：\n\
         - claimType ∈ {{factual, definitional, causal, comparative, evaluative, predictive, normative, hypothetical}}\n\
         - polarity ∈ {{positive, negative}}\n\
         - modality ∈ {{asserted, possible, probable, capable, necessary, recommended}}；\
         普通事实陈述一律用 asserted（不要输出 \"is\"/\"are\" 等系词）。\n\
         只输出一个 JSON 对象，形如 {{\"claims\":[ ... ]}}，不要任何解释或 Markdown。\
         若文本无可抽取的 Claim，输出 {{\"claims\":[]}}。"
    )
}

/// 用给定 system/user 执行一次模型抽取，返回已通过受控词表校验的候选 Claim。
///
/// 与 [`extract_claims`] 共用同一套谓语校验逻辑；后台 `extraction_service`
/// 按块分批调用它以获得真实的分块进度（`sourceChunk` 仍回指绝对块下标）。
pub fn extract_corpus(
    conn: &Connection,
    system: String,
    user: String,
) -> AppResult<Vec<ExtractedClaim>> {
    let request = CompletionRequest::structured(system, user);
    crate::log_info!("抽取：发起一次模型补全");
    let response = default_provider(conn).complete(&request)?;

    // 宽松解析：容忍模型偶尔加上的 Markdown 围栏或前后解释文字，
    // 同时兼容「对象包裹 {\"claims\":[...]}」与「裸数组 [...]」两种返回形态。
    let raw: Vec<RawClaim> = parse_claims(&response.text).map_err(|err| {
        crate::log_error!(
            "模型返回内容无法解析为 Claim JSON：{err}；原文（前 800 字符）：{}",
            crate::logging::clip(&response.text, 800)
        );
        AppError::Internal(format!("AI 返回的 JSON 解析失败：{err}"))
    })?;
    crate::log_info!("抽取：解析出 {} 条候选 Claim", raw.len());

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

    Ok(extracted)
}

/// 每批送进模型的硬上限：块数与累计字符双约束。
///
/// 输出 JSON 的体量与输入语料正相关；单次输入过大时输出会撞上
/// `max_tokens` 上限被截断，JSON 解析必然失败（典型报错：
/// `EOF while parsing a string`）。
pub const MAX_BATCH_CHUNKS: usize = 4;
pub const MAX_BATCH_CHARS: usize = 6000;

/// 把切片按预算分组成多批，每批是 `(chunk_index, content)` 列表。
///
/// 供同步 [`extract_claims`] 与后台 `extraction_service` 共用，
/// 保证两条路径的单次调用体量一致、可预期。
pub fn batch_chunks(chunks: &[Chunk]) -> Vec<Vec<(usize, String)>> {
    let mut batches: Vec<Vec<(usize, String)>> = Vec::new();
    let mut current: Vec<(usize, String)> = Vec::new();
    let mut current_chars = 0usize;
    for chunk in chunks {
        let len = chunk.content.chars().count();
        if !current.is_empty()
            && (current.len() >= MAX_BATCH_CHUNKS || current_chars + len > MAX_BATCH_CHARS)
        {
            batches.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current.push((chunk.chunk_index, chunk.content.clone()));
        current_chars += len;
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
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

    // 先尝试整段解析；失败再退回切片提取，兼容围栏/杂散文字。
    let value: Option<serde_json::Value> = {
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => Some(v),
            Err(_) => {
                if let (Some(s), Some(e)) = (trimmed.find('['), trimmed.rfind(']')) {
                    serde_json::from_str(&trimmed[s..=e]).ok()
                } else if let (Some(s), Some(e)) = (trimmed.find('{'), trimmed.rfind('}')) {
                    serde_json::from_str(&trimmed[s..=e]).ok()
                } else {
                    None
                }
            }
        }
    };

    let arr = match value {
        Some(serde_json::Value::Array(a)) => a,
        Some(serde_json::Value::Object(ref map)) => map
            .get("claims")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => {
            // 整段解析失败——最典型的是输出被 max_tokens 截断
            // （`{"claims":[{...},{..`）。抢救出其中语法完整的对象，
            // 已完成的 Claim 不浪费；一个都救不出才向上报错。
            let salvaged = salvage_objects(trimmed);
            if salvaged.is_empty() {
                return Err(serde_json::from_str::<serde_json::Value>(trimmed).unwrap_err());
            }
            crate::log_warn!(
                "模型返回的 JSON 不完整（疑似被截断），抢救出 {} 个完整对象",
                salvaged.len()
            );
            let flat: Vec<serde_json::Value> = salvaged
                .into_iter()
                .map(|v| match v.get("claims") {
                    Some(c) => c.as_array().cloned().unwrap_or_default(),
                    None => vec![v],
                })
                .collect::<Vec<_>>()
                .into_iter()
                .flatten()
                .collect();
            return serde_json::from_value(serde_json::Value::Array(flat));
        }
    };

    serde_json::from_value(serde_json::Value::Array(arr))
}

/// 用状态机扫描文本，把其中**语法完整**的 `{...}` 对象逐个抠出来。
///
/// 逐字符跟踪字符串/转义/配平：只有花括号真正配平闭合的对象才收录，
/// 被截断的最后一个残缺对象会被自然丢弃。截断常发生在外层
/// `{"claims":[…` 尚未闭合时，因此收录后再丢弃"包含其他对象的祖先"
/// （未闭合的 wrapper 本身不会入选，已闭合但包裹了候选的祖先也一并让位），
/// 保证救出来的是 Claim 对象本身。
fn salvage_objects(text: &str) -> Vec<serde_json::Value> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<(usize, usize)> = Vec::new(); // (start, end) 已配平的对象区间
    let mut stack: Vec<usize> = Vec::new(); // 未闭合的 '{' 下标
    let mut in_string = false;
    let mut escaped = false;

    for (i, &ch) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push(i),
            '}' => {
                if let Some(start) = stack.pop() {
                    spans.push((start, i));
                }
            }
            _ => {}
        }
    }

    // 丢弃包含其他配平对象的祖先：它要么是外层 wrapper（其中的候选才是我们要的），
    // 要么是模型多套的一层结构。区间两两不等的包含关系按闭区间判断。
    let keep: Vec<bool> = spans
        .iter()
        .map(|&(s, e)| {
            !spans
                .iter()
                .any(|&(s2, e2)| (s2, e2) != (s, e) && s2 >= s && e2 <= e)
        })
        .collect();

    spans
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .filter_map(|(&(s, e), _)| {
            let obj: String = chars[s..=e].iter().collect();
            serde_json::from_str::<serde_json::Value>(&obj).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_claims_accepts_complete_object_wrapper() {
        let text = r#"{"claims":[{"subject":"Rust","predicate":"enables","objectText":"安全并发"}]}"#;
        let claims = parse_claims(text).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "Rust");
    }

    #[test]
    fn parse_claims_salvages_complete_objects_from_truncated_output() {
        // 模拟被 max_tokens 截断：最后一个对象在字符串中途被切断。
        let text = r#"{"claims":[{"subject":"Rust","predicate":"enables","objectText":"安全并发"},{"subject":"Tauri","predicate":"provides","objectText":"WebView"#;
        let claims = parse_claims(text).expect("应从截断输出中抢救出完整对象");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "Rust");
    }

    #[test]
    fn salvage_respects_escaped_quotes_inside_strings() {
        // 字符串内的 \" 不能被当成字符串结束，否则对象边界会算错。
        let text = r#"{"claims":[{"subject":"he said \"hi\"","predicate":"enables","objectText":"x"},{"sub"#;
        let claims = parse_claims(text).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "he said \"hi\"");
    }

    #[test]
    fn parse_claims_errors_when_nothing_salvageable() {
        let text = "完全不是 JSON 的话";
        assert!(parse_claims(text).is_err());
    }

    #[test]
    fn batch_chunks_respects_char_budget_and_chunk_cap() {
        let make = |idx: usize, len: usize| Chunk {
            id: crate::domain::common::ids::ChunkId::from_raw(&format!("c{idx}")),
            document_id: DocumentId::from_raw("d"),
            chunk_index: idx,
            start_offset: 0,
            end_offset: 0,
            content: "字".repeat(len),
            char_count: len,
        };
        let chunks: Vec<Chunk> = vec![make(0, 4000), make(1, 4000), make(2, 100), make(3, 100)];
        let batches = batch_chunks(&chunks);
        // 第 0、1 块相加 8000 > 6000 → 拆开；第 1+2+3 块共 4200 ≤ 6000 → 合成一批。
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 1);
        assert_eq!(batches[1].len(), 3);
    }
}
