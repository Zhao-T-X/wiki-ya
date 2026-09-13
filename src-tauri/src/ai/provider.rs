//! AI Provider 抽象 —— 所有 LLM 调用都收敛到这一个 trait。
//!
//! 设计要点：
//! - `Provider` 是对象安全的，便于按配置注入（离线 / OpenAI 兼容）。
//! - `complete` 是唯一的模型调用入口，入参是朴素的「系统提示 + 用户提示 + 开关」，
//!   不暴露任何供应商特有的魔法字段，未来换供应商只改本文件。
//! - 离线 provider 永远 `enabled() == false`，仅用于「未配置 Key」时的诚实降级，
//!   不返回任何编造内容。

use rusqlite::Connection;

use crate::ai::config::AiConfig;
use crate::error::{AppError, AppResult};

/// 一次补全请求。
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system: String,
    pub user: String,
    /// 是否要求模型以 JSON 对象形式返回（供应商需支持 response_format）。
    pub json_mode: bool,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl CompletionRequest {
    /// 构造一个偏结构化的抽取请求（要求模型输出 JSON、低温）。
    ///
    /// 这里**不**强制 `response_format: json_object`，原因有二：
    /// 1) 许多 OpenAI 兼容端点 / 推理模型（DeepSeek 推理系等）在 json_object
    ///    约束下会返回空 content —— 把预算全花在 reasoning_content 上；
    /// 2) 我们已用「提示词要求 JSON + 容错解析（parse_claims）」兜底，不需要
    ///    依赖供应商的 response_format 特性，换取最大兼容性。
    pub fn structured(system: String, user: String) -> Self {
        CompletionRequest {
            system,
            user,
            json_mode: false,
            temperature: 0.2,
            // 给推理模型留足预算：reasoning 与最终答案共享该上限，太小会导致 content 为空。
            max_tokens: 8192,
        }
    }

    /// 构造一个偏自由的问答请求。
    pub fn chat(system: String, user: String) -> Self {
        CompletionRequest {
            system,
            user,
            json_mode: false,
            temperature: 0.3,
            max_tokens: 1024,
        }
    }
}

/// 一次补全的返回。
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub text: String,
    pub model: String,
}

/// LLM 供应商抽象。
pub trait Provider: Send + Sync {
    /// 供应商名（用于上报与诊断，如 `openai-compatible` / `offline`）。
    fn name(&self) -> &'static str;
    /// 当前是否可用（无 Key 即不可用）。
    fn enabled(&self) -> bool;
    /// 执行一次补全。不可用时应返回错误，绝不留空或编造。
    fn complete(&self, request: &CompletionRequest) -> AppResult<CompletionResponse>;

    /// 流式补全：逐段回调文本增量，最终返回完整文本。
    ///
    /// 默认不支持流式（诚实报错，不假装）；支持 SSE 的 Provider 覆盖此方法。
    fn complete_streaming(
        &self,
        request: &CompletionRequest,
        on_delta: &dyn Fn(&str),
    ) -> AppResult<CompletionResponse> {
        let _ = (request, on_delta);
        Err(AppError::Internal("当前 Provider 不支持流式输出".into()))
    }

    /// 向量化：把一批文本转成 embedding。不可用时应返回错误。
    fn embed(&self, texts: &[String]) -> AppResult<Vec<Vec<f32>>>;
}

/// 离线 provider：从不联网，仅用于「未配置 Key」时的诚实降级。
pub struct OfflineProvider;

impl Provider for OfflineProvider {
    fn name(&self) -> &'static str {
        "offline"
    }

    fn enabled(&self) -> bool {
        false
    }

    fn complete(&self, _request: &CompletionRequest) -> AppResult<CompletionResponse> {
        Err(AppError::Internal(
            "AI 未启用：未配置 WIKIYA_API_KEY。配置后重启应用即可启用。".into(),
        ))
    }

    fn embed(&self, _texts: &[String]) -> AppResult<Vec<Vec<f32>>> {
        Err(AppError::Internal(
            "AI 未启用：未配置 WIKIYA_API_KEY，无法进行向量化。".into(),
        ))
    }
}

/// OpenAI 兼容 provider（也兼容任意 `/chat/completions` 端点，如本地推理服务）。
pub struct OpenAiProvider {
    config: AiConfig,
}

impl OpenAiProvider {
    pub fn new(config: AiConfig) -> Self {
        OpenAiProvider { config }
    }
}

#[derive(serde::Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    /// 关闭"思考"（推理）模式。多款 OpenAI 兼容端点（Qwen / 部分 DeepSeek 代理等）
    /// 支持该字段；仅在检测到推理模型把预算耗在 reasoning 上时才附带发送，
    /// 避免对不支持的端点造成干扰（不支持时会返回 4xx，被重试逻辑跳过）。
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
}

#[derive(serde::Serialize, Clone)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(serde::Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(serde::Deserialize)]
struct Choice {
    message: ChatMessageOut,
    /// `stop` 表示正常结束；`length` 表示被 max_tokens 截断（推理吃满预算的典型信号）。
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct ChatMessageOut {
    /// 部分推理模型把正文放在 `reasoning_content`，`content` 可能为 null/空，
    /// 这里用 Option 兼容，并在上层决定是否报错或回退。
    #[serde(default)]
    content: Option<String>,
    /// 兼容 DeepSeek-R1 等推理模型的思维链字段（仅诊断用）。
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(serde::Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(serde::Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(serde::Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

// SSE 流式 chunk（OpenAI 兼容格式）：`data: {"choices":[{"delta":{"content":"..."}}]}`。
#[derive(serde::Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(serde::Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    /// 推理模型的思维链增量；正文可能为空而答案藏在这里。
    #[serde(default)]
    reasoning_content: Option<String>,
}

impl Provider for OpenAiProvider {
    fn name(&self) -> &'static str {
        "openai-compatible"
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn complete(&self, request: &CompletionRequest) -> AppResult<CompletionResponse> {
        let api_key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| AppError::Internal("AI 未配置 API Key".into()))?;

        let client = reqwest::blocking::Client::new();
        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));

        // 推理模型（如 DeepSeek-R1/Flash）会把 token 预算花在 reasoning_content 上，
        // 导致 content 为空甚至被 max_tokens 截断（finish_reason=length）。
        // 因此空 content 时按 (是否 json 模式, max_tokens, 是否关闭思考) 依次升级重试：
        //   1) 原样；
        //   2) 大幅提高预算，让推理有机会写完并落到 content；
        //   3) 关闭「思考」模式（推理模型专用，支持该字段的端点会把答案直接写进 content）。
        // 正常情况下第 1 次就成功，后续组合只在失败路径上消耗，不影响常规成本。
        const HIGH_BUDGET: u32 = 32_768;
        let mut attempts: Vec<(bool, u32, bool)> = Vec::new();
        let json_flags: &[bool] = if request.json_mode { &[true, false] } else { &[false] };
        for &use_json in json_flags {
            attempts.push((use_json, request.max_tokens, false));
            attempts.push((use_json, HIGH_BUDGET, false));
            // 关闭思考时保持一个相对保守的预算：既给答案留空间，也降低被端点 4xx 拒绝的概率。
            attempts.push((use_json, request.max_tokens.max(8_192), true));
            attempts.push((use_json, HIGH_BUDGET, true));
        }

        let mut last_diag: Option<String> = None;
        for (use_json, max_tokens, disable_thinking) in attempts {
            let response_format = if use_json {
                Some(ResponseFormat {
                    kind: "json_object".into(),
                })
            } else {
                None
            };

            let body = ChatRequest {
                model: self.config.model.clone(),
                messages: vec![
                    ChatMessage {
                        role: "system".into(),
                        content: request.system.clone(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: request.user.clone(),
                    },
                ],
                temperature: request.temperature,
                max_tokens,
                response_format,
                stream: None,
                enable_thinking: disable_thinking.then_some(false),
            };

            crate::log_debug!(
                "AI 请求 → POST {url} model={} json_mode={} max_tokens={max_tokens} \
                 disable_thinking={disable_thinking} temp={} system_chars={} user_chars={}",
                self.config.model,
                use_json,
                request.temperature,
                request.system.chars().count(),
                request.user.chars().count()
            );
            if crate::logging::enabled(crate::logging::Level::Trace) {
                // 仅在 trace 级别序列化请求体，避免正常路径的额外开销。
                let body_json = serde_json::to_string(&body).unwrap_or_default();
                crate::log_trace!("AI 请求体：{}", crate::logging::clip(&body_json, 2000));
            }

            let response = client
                .post(&url)
                .bearer_auth(api_key.as_str())
                .json(&body)
                .send()
                .map_err(|err| {
                    crate::log_error!("AI 请求失败：{err}");
                    AppError::Internal(format!("AI 请求失败：{err}"))
                })?;

            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_string();
            let content_length = response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_string();
            crate::log_debug!(
                "AI 响应 ← status={} content-type={content_type} content-length={content_length} url={}",
                status.as_u16(),
                response.url()
            );

            if !status.is_success() {
                let text = response.text().unwrap_or_default();
                let code = status.as_u16();
                crate::log_warn!(
                    "AI 接口返回 {status}：{}",
                    crate::logging::clip(&text, 400)
                );
                // 认证类错误重试无意义，直接失败；其余（例如端点不支持更大的
                // max_tokens 或不认识 enable_thinking 而报 400）应继续尝试下一组合。
                if code == 401 || code == 403 {
                    return Err(AppError::Internal(format!("AI 接口返回 {status}：{text}")));
                }
                last_diag = Some(format!(
                    "HTTP {status}：{}",
                    crate::logging::clip(&text, 300)
                ));
                continue;
            }

            // 先读原始文本，便于在空响应时把片段回显到诊断信息，方便排查配置问题。
            let raw = response.text().unwrap_or_default();
            crate::log_debug!(
                "AI 响应体（{} 字符）：{}",
                raw.chars().count(),
                crate::logging::clip(&raw, 800)
            );
            let parsed: ChatResponse = match serde_json::from_str(&raw) {
                Ok(parsed) => parsed,
                Err(err) => {
                    // 空 body / 非 JSON（如 200+空、HTML 错误页、把 SSE 当非流式读）：
                    // 不能直接 `?` 中止——否则会跳过后续重试与「流式兜底」，
                    // 而这类端点恰恰常常只在流式下才回正文。
                    crate::log_warn!(
                        "AI 响应无法解析为 JSON：{err}（HTTP {}，响应体 {} 字符）",
                        status.as_u16(),
                        raw.chars().count()
                    );
                    last_diag = Some(format!(
                        "JSON 解析失败：{err}（HTTP {}，响应体 {} 字符：{}）",
                        status.as_u16(),
                        raw.chars().count(),
                        crate::logging::clip(&raw, 400)
                    ));
                    continue;
                }
            };

            let choice_count = parsed.choices.len();
            let finish_reason = parsed
                .choices
                .first()
                .and_then(|c| c.finish_reason.clone());
            let (content, reasoning) = parsed
                .choices
                .into_iter()
                .next()
                .map(|c| (c.message.content, c.message.reasoning_content))
                .unwrap_or_default();
            crate::log_debug!(
                "AI 响应解析：choices={choice_count} finish_reason={} content={} 字符 reasoning={} 字符",
                finish_reason.as_deref().unwrap_or("-"),
                content.as_deref().map(|s| s.chars().count()).unwrap_or(0),
                reasoning.as_deref().map(|s| s.chars().count()).unwrap_or(0),
            );

            let text = content.unwrap_or_default();
            if !text.trim().is_empty() {
                crate::log_debug!("AI 抽取成功：content {} 字符", text.chars().count());
                return Ok(CompletionResponse {
                    text,
                    model: self.config.model.clone(),
                });
            }

            // content 为空：先尝试从 reasoning_content 里捞出最终 JSON
            //（部分推理模型 / 代理把答案追加在思维链末尾，且 content 恒为空）。
            if let Some(json) = reasoning.as_deref().and_then(extract_claims_json) {
                crate::log_info!(
                    "content 为空，但从 reasoning_content 提取到 Claim JSON（{} 字符），采用",
                    json.chars().count()
                );
                return Ok(CompletionResponse {
                    text: json,
                    model: self.config.model.clone(),
                });
            }

            // 记诊断：附带 finish_reason 与 reasoning 末尾，便于判断「被截断」还是「答案在后半段」。
            let mut diag = format!(
                "空 content（choices 数={choice_count}，max_tokens={max_tokens}，finish_reason={}）",
                finish_reason.as_deref().unwrap_or("-")
            );
            if let Some(reasoning) = reasoning.as_ref().filter(|r| !r.trim().is_empty()) {
                diag.push_str(&format!(
                    "；reasoning_content {} 字符，末尾：{}",
                    reasoning.chars().count(),
                    crate::logging::clip_tail(reasoning, 200)
                ));
            } else {
                let snippet: String = raw.chars().take(400).collect();
                if !snippet.trim().is_empty() {
                    diag.push_str(&format!("；原始响应片段：{snippet}"));
                }
            }
            crate::log_warn!("本次请求未返回可用 content：{diag}");
            last_diag = Some(diag);
        }

        // 兜底：部分推理 / 代理端点仅在**流式**模式下才吐出正文，非流式恒为空。
        {
            crate::log_info!("非流式均未拿到 content，尝试流式兜底……");
            let mut stream_req = request.clone();
            stream_req.json_mode = false;
            stream_req.max_tokens = request.max_tokens.max(HIGH_BUDGET);
            match self.complete_streaming(&stream_req, &|_| {}) {
                Ok(resp) if !resp.text.trim().is_empty() => {
                    crate::log_info!("流式兜底成功：{} 字符", resp.text.chars().count());
                    return Ok(resp);
                }
                Ok(_) => crate::log_warn!("流式兜底返回内容仍为空"),
                Err(err) => crate::log_warn!("流式兜底失败：{err}"),
            }
        }

        let diag = last_diag.unwrap_or_else(|| "（无诊断信息）".into());
        // 若诊断显示是"推理模型把预算耗光 / 被截断"，给出可执行的建议而非让用户猜。
        let hint = if diag.contains("finish_reason=length") || diag.contains("reasoning_content") {
            " 建议：这是推理型模型的典型症状（思维链耗尽输出预算）。\
             请在 Settings → AI 运行时把 Chat 模型换成**非推理模型**（如 \
             `deepseek-chat` / `gpt-4o-mini`）后重试。"
        } else {
            ""
        };
        crate::log_error!("AI 抽取彻底失败：{diag}");
        Err(AppError::Internal(format!(
            "AI 响应内容为空：模型未返回任何文本。{diag}{hint}"
        )))
    }

    fn embed(&self, texts: &[String]) -> AppResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let api_key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| AppError::Internal("AI 未配置 API Key".into()))?;

        let body = EmbeddingRequest {
            model: self.config.embedding_model.clone(),
            input: texts.to_vec(),
        };

        let client = reqwest::blocking::Client::new();
        let url = format!("{}/embeddings", self.config.base_url.trim_end_matches('/'));
        let response = client
            .post(url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .map_err(|err| AppError::Internal(format!("向量化请求失败：{err}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            return Err(AppError::Internal(format!("向量化接口返回 {status}：{text}")));
        }

        let parsed: EmbeddingResponse = response
            .json()
            .map_err(|err| AppError::Internal(format!("向量化响应解析失败：{err}")))?;

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    /// SSE 流式补全：`stream: true` 逐行读 `data:` 帧，把 `delta.content`
    /// 增量回调给调用方。兼容任意 OpenAI 兼容端点（含本地推理服务）。
    fn complete_streaming(
        &self,
        request: &CompletionRequest,
        on_delta: &dyn Fn(&str),
    ) -> AppResult<CompletionResponse> {
        use std::io::{BufRead, BufReader};

        let api_key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| AppError::Internal("AI 未配置 API Key".into()))?;

        let body = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: request.system.clone(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: request.user.clone(),
                },
            ],
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            response_format: None,
            stream: Some(true),
            // 流式是 Ask/Research 的主路径：绝不附带供应商特有字段，
            // 否则严格端点（如 OpenAI）会因未知参数直接 400。
            enable_thinking: None,
        };

        let client = reqwest::blocking::Client::new();
        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        crate::log_debug!(
            "AI 流式请求 → POST {url} model={} max_tokens={}",
            self.config.model,
            request.max_tokens
        );
        let response = client
            .post(&url)
            .bearer_auth(api_key.as_str())
            .json(&body)
            .send()
            .map_err(|err| {
                crate::log_error!("AI 流式请求失败：{err}");
                AppError::Internal(format!("AI 流式请求失败：{err}"))
            })?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();
        crate::log_debug!(
            "AI 流式响应 ← status={} content-type={content_type} url={}",
            status.as_u16(),
            response.url()
        );
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            crate::log_error!("AI 流式接口返回 {status}：{}", crate::logging::clip(&text, 800));
            return Err(AppError::Internal(format!("AI 接口返回 {status}：{text}")));
        }

        let reader = BufReader::new(response);
        let mut full = String::new();
        let mut reasoning_full = String::new();
        let mut finish_reason: Option<String> = None;
        let mut frames = 0usize;
        for line in reader.lines() {
            let line = line.map_err(|err| AppError::Internal(format!("流式读取失败：{err}")))?;
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) else {
                crate::log_trace!("忽略非 JSON 流式帧：{}", crate::logging::clip(data, 200));
                continue; // 容忍心跳/注释帧
            };
            let Some(choice) = chunk.choices.into_iter().next() else {
                continue;
            };
            if let Some(reason) = choice.finish_reason {
                finish_reason = Some(reason);
            }
            let delta = choice.delta;
            if let Some(content) = delta.content {
                if !content.is_empty() {
                    frames += 1;
                    on_delta(&content);
                    full.push_str(&content);
                }
            }
            // 推理模型可能只吐 reasoning_content（content 恒空），一并累积备用。
            if let Some(reasoning) = delta.reasoning_content {
                reasoning_full.push_str(&reasoning);
            }
        }

        crate::log_debug!(
            "AI 流式完成：content 帧 {frames} 个，content {} 字符，reasoning {} 字符，finish_reason={}",
            full.chars().count(),
            reasoning_full.chars().count(),
            finish_reason.as_deref().unwrap_or("-")
        );

        if full.is_empty() {
            // 正文为空但推理有内容：尝试从 reasoning_content 里捞出 Claim JSON。
            if let Some(json) = extract_claims_json(&reasoning_full) {
                crate::log_info!(
                    "流式仅收到 reasoning_content，从中提取到 Claim JSON（{} 字符）",
                    json.chars().count()
                );
                return Ok(CompletionResponse {
                    text: json,
                    model: self.config.model.clone(),
                });
            }
            crate::log_warn!(
                "AI 流式响应为空（无 content 增量；reasoning {} 字符，末尾：{}）",
                reasoning_full.chars().count(),
                crate::logging::clip_tail(&reasoning_full, 300)
            );
            return Err(AppError::Internal("AI 流式响应为空".into()));
        }

        Ok(CompletionResponse {
            text: full,
            model: self.config.model.clone(),
        })
    }
}

/// 从任意文本里捞出「最像 Claim JSON」的片段。
///
/// 用途：某些推理模型 / 代理会把正文（含最终 JSON）一股脑塞进 `reasoning_content`，
/// 且 `content` 恒为空。此时不能要求整段都是 JSON，而应扫描其中**平衡的**
/// `{...}` / `[...]` 候选，取最后一个能解析且形如 Claim 的片段（最终答案通常在末尾）。
fn extract_claims_json(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<u8> = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, b) in text.bytes().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                if stack.is_empty() {
                    start = i;
                }
                stack.push(b);
            }
            b'}' | b']' => {
                if let Some(open) = stack.pop() {
                    let matched = (open == b'{' && b == b'}') || (open == b'[' && b == b']');
                    if !matched {
                        stack.clear();
                        in_string = false;
                        escaped = false;
                        continue;
                    }
                    if stack.is_empty() {
                        candidates.push((start, i));
                    }
                }
            }
            _ => {}
        }
    }

    // 从后往前：末尾的 JSON 最可能是最终答案。
    for (s, e) in candidates.into_iter().rev() {
        let slice = &text[s..=e];
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(slice) {
            // 收紧判据，避免误把思维链里随手举例的数组/对象当答案：
            // - 数组：必须是非空、且首元素为对象（Claim 候选列表）；
            // - 对象：必须带 `claims` 数组字段。
            let looks_like_claims = match &value {
                serde_json::Value::Array(items) => {
                    items.first().map(|v| v.is_object()).unwrap_or(false)
                }
                serde_json::Value::Object(map) => {
                    map.get("claims").map(|c| c.is_array()).unwrap_or(false)
                }
                _ => false,
            };
            if looks_like_claims {
                return Some(slice.to_string());
            }
        }
    }
    None
}

/// 按当前环境配置选择 provider。
///
/// 有 Key → `OpenAiProvider`；否则 → `OfflineProvider`（诚实降级）。
pub fn default_provider(conn: &Connection) -> Box<dyn Provider> {
    let config = AiConfig::from_settings(conn);
    if config.enabled {
        Box::new(OpenAiProvider::new(config))
    } else {
        Box::new(OfflineProvider)
    }
}
