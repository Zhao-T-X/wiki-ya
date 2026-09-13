//! IPC 数据传输对象 —— 前端与 Rust 之间的**唯一**形状约定。
//!
//! 一一对应 `docs/IPC契约.md`。改这里之前先改契约文档。
//!
//! 命名策略：Rust 侧全员 `snake_case`，序列化时统一 `camelCase`，
//! 因此 TS 侧是符合直觉的 camelCase，而 Rust 侧不需要为字段名打架。
//!
//! 边界原则：**DTO 不使用领域枚举作为类型**，而用 `String`。
//! 原因有两点：前端本来就要按字符串渲染与筛选；更关键的是，
//! 领域枚举的 `Deserialize` 会拒绝未知值，一旦数据库里有历史遗留值，
//! 整个接口会 500。枚举合法性在**写入**路径上严格把关，
//! 读取路径则宽容透传，让 UI 有机会展示它。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 元信息
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub db_path: String,
    pub schema_version: i64,
    pub registry_version: String,
    /// Phase 6 之前恒为 false —— UI 必须如实展示，不得伪造 AI 能力。
    pub ai_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeOption {
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationPredicateOption {
    pub predicate: String,
    pub inverse_label: String,
    pub symmetric: bool,
    pub transitive: bool,
    pub source_types: Vec<String>,
    pub target_types: Vec<String>,
}

/// 受控词表总览。前端的下拉与校验提示全部来自这里，不硬编码。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registries {
    pub entity_types: Vec<EntityTypeOption>,
    pub claim_predicates: Vec<String>,
    pub relation_predicates: Vec<RelationPredicateOption>,
    pub claim_types: Vec<String>,
    pub polarities: Vec<String>,
    pub modalities: Vec<String>,
    pub claim_statuses: Vec<String>,
    pub entity_statuses: Vec<String>,
    pub relation_statuses: Vec<String>,
    pub idea_statuses: Vec<String>,
    pub question_statuses: Vec<String>,
    pub question_types: Vec<String>,
    pub event_types: Vec<String>,
    pub event_statuses: Vec<String>,
    pub event_time_precisions: Vec<String>,
    pub research_task_statuses: Vec<String>,
    pub source_types: Vec<String>,
    pub claim_relation_types: Vec<String>,
    pub claim_relation_statuses: Vec<String>,
    pub normalization_outcomes: Vec<String>,
    pub entity_resolution_steps: Vec<String>,
    pub load_strategies: Vec<String>,
    pub agent_roles: Vec<String>,
    pub registry_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub total_documents: i64,
    pub total_chunks: i64,
    pub total_entities: i64,
    pub total_claims: i64,
    pub total_evidence: i64,
    pub potential_duplicates: i64,
    pub unresolved_conflicts: i64,
    pub claims_without_evidence: i64,
    pub unresolved_entities: i64,
    pub superseded_claims: i64,
}

// ---------------------------------------------------------------------------
// Document
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDocumentInput {
    pub title: String,
    pub content: String,
    /// 缺省为 `note`；非法取值会返回 `DOMAIN_RULE_VIOLATION`。
    #[serde(default)]
    pub source_type: Option<String>,
    #[serde(default)]
    pub source_uri: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub source_type: String,
    pub source_uri: Option<String>,
    pub content_hash: String,
    pub chunk_count: i64,
    /// 正文字符数（不是字节数）。
    pub char_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkCard {
    pub id: String,
    pub document_id: String,
    pub chunk_index: i64,
    pub start_offset: i64,
    pub end_offset: i64,
    pub content: String,
    pub char_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDetail {
    pub document: DocumentSummary,
    pub content: String,
    pub chunks: Vec<ChunkCard>,
    pub claims: Vec<ClaimCard>,
}

// ---------------------------------------------------------------------------
// Knowledge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityCard {
    pub id: String,
    pub name: String,
    pub primary_type: String,
    pub types: Vec<String>,
    pub description: Option<String>,
    pub status: String,
    pub alias_count: i64,
    pub claim_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationCard {
    pub id: String,
    pub source_id: String,
    pub source_name: String,
    pub predicate: String,
    pub inverse_label: Option<String>,
    pub target_id: String,
    pub target_name: String,
    pub confidence: Option<f32>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub type_name: String,
    pub status: String,
    pub depth: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub predicate: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPayload {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// 是否因为节点上限被截断 —— UI 要如实告知，不能让用户以为看到了全图。
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityDetail {
    pub entity: EntityCard,
    pub aliases: Vec<String>,
    pub claims: Vec<ClaimCard>,
    pub relations: Vec<RelationCard>,
    pub graph: GraphPayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimCard {
    pub id: String,
    pub subject_id: String,
    pub subject_name: String,
    pub predicate: String,
    pub object_id: Option<String>,
    pub object_name: Option<String>,
    pub object_text: Option<String>,
    pub content: Option<String>,
    pub claim_type: String,
    pub polarity: String,
    pub modality: String,
    pub condition: Option<String>,
    pub confidence: Option<f32>,
    pub status: String,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub source_document_id: Option<String>,
    pub source_document_title: Option<String>,
    pub source_quote: Option<String>,
    pub evidence_count: i64,
    pub created_at: String,
    /// `current` | `superseded` | `excluded` —— 派生值，不落库（见领域文档 §4.7）。
    /// `excluded` 表示该 Claim 不参与检索（rejected / archived / draft），
    /// 与「历史（superseded）」是两回事。
    pub lifecycle: String,
    /// 可读陈述（抽取没给 content 时由主语/谓语/宾语拼出）。
    pub display_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceCard {
    pub id: String,
    pub document_id: String,
    pub document_title: Option<String>,
    pub chunk_id: Option<String>,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
    pub quote: Option<String>,
    pub evidence_level: u8,
    pub evidence_level_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimDetail {
    pub claim: ClaimCard,
    pub evidence: Vec<EvidenceCard>,
    pub relations: Vec<ClaimRelationCard>,
    pub history: Vec<ClaimRelationCard>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClaimInput {
    pub subject: String,
    pub predicate: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub claim_type: Option<String>,
    #[serde(default)]
    pub polarity: Option<String>,
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
    pub document_id: String,
    #[serde(default)]
    pub chunk_id: Option<String>,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchInput {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    /// Phase 6 前恒为降级：请求 true 也只返回 `lexical`，但**不报错**。
    #[serde(default)]
    pub semantic: Option<bool>,
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
    pub method: String,
    pub matched_in: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub took_ms: u64,
    pub method: String,
    pub total: i64,
    pub hits: Vec<SearchHit>,
    /// 语义检索未启用时的说明（如 "语义检索需要 AI Runtime（Phase 6）"）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

// ---------------------------------------------------------------------------
// Evolution / Review
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRelationCard {
    pub id: String,
    pub source_claim_id: String,
    pub source_text: String,
    pub target_claim_id: String,
    pub target_text: String,
    pub relationship: String,
    pub status: String,
    pub confidence: Option<f32>,
    pub reason: Option<String>,
    pub suggested_action: Option<String>,
    pub target_previous_status: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    pub document_id: String,
    pub claims_scanned: i64,
    pub relations_written: i64,
    pub duplicates: i64,
    pub coexists: i64,
    pub contradictions: i64,
    pub needs_review: i64,
    pub verdicts: Vec<ClaimRelationCard>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub relation: ClaimRelationCard,
    pub priority: u8,
    pub what_changed: String,
    pub why: String,
    pub evidence_quote: Option<String>,
    pub impact: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideRelationInput {
    pub relation_id: String,
    /// `accept` | `reject` | `reset`
    pub decision: String,
    #[serde(default)]
    pub relationship: Option<String>,
}

// ---------------------------------------------------------------------------
// 通用入参
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListDocumentsInput {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdInput {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListEntitiesInput {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub entity_type: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetEntityInput {
    pub id: String,
    #[serde(default)]
    pub depth: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListClaimsInput {
    #[serde(default)]
    pub subject_id: Option<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LimitInput {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeDocumentInput {
    pub document_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEvidenceInput {
    pub claim_id: String,
}

// ---------------------------------------------------------------------------
// AI 抽取（Phase 5）
// ---------------------------------------------------------------------------

/// 单条被抽取的 Claim 候选（预览用，不落库）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedClaim {
    pub subject: String,
    pub predicate: String,
    pub object_text: Option<String>,
    pub content: Option<String>,
    pub claim_type: Option<String>,
    pub polarity: Option<String>,
    pub modality: Option<String>,
    pub confidence: Option<f32>,
    pub source_chunk_index: Option<usize>,
    pub source_quote: Option<String>,
    pub sentence: Option<String>,
    /// 是否通过受控词表校验；false 表示 predicate 不合法，已被排除。
    pub accepted: bool,
    pub reject_reason: Option<String>,
}

/// 一篇文档的抽取报告。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionReport {
    pub document_id: String,
    /// 实际使用的 provider 名（如 `openai-compatible` / `offline`）。
    pub provider: String,
    /// 本次抽取是否真的跑了模型；false 时 `extracted` 必为空。
    pub enabled: bool,
    /// 未启用 / 空结果等说明，UI 原样展示。
    pub note: Option<String>,
    pub extracted: Vec<ExtractedClaim>,
}

// ---------------------------------------------------------------------------
// Ask 问答（Phase 6）
// ---------------------------------------------------------------------------

/// 提问入参。`role` 缺省为 `auto`（由后端选 KnowledgeAgent）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRequest {
    pub question: String,
    #[serde(default)]
    pub role: Option<String>,
    /// 前端生成的本次运行 id，用于关联流式事件（TDD §53）；缺省不推送事件。
    #[serde(default)]
    pub run_id: Option<String>,
}

/// 一条来源（与 prompt 中的 [n] 编号对齐），可逐级下钻 Source → Chunk → Claim。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSource {
    pub index: usize,
    pub kind: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
}

/// 一次问答的诚实结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskResponse {
    pub question: String,
    pub answer: String,
    /// 是否真的跑了模型；false 时 `answer` 为空、`sources` 为空。
    pub enabled: bool,
    /// 未启用 / 截断等说明，UI 如实展示。
    pub note: Option<String>,
    pub sources: Vec<AskSource>,
    /// 上下文统计（预算 / 命中 / 截断），透明可审计（TDD §65）。
    pub context_stats: Option<crate::ai::context::ContextStats>,
}

// ---------------------------------------------------------------------------
// 2.0 AI 运行时设置（Phase 6 配置化）
// ---------------------------------------------------------------------------

/// 当前 AI 运行时设置（不含明文 API Key，仅告知是否已配置，出于安全）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    /// 是否已配置 API Key（不回显明文）。
    pub api_key_set: bool,
    pub base_url: String,
    pub model: String,
    pub embedding_model: String,
    /// Ask/Research 的上下文预算（token）。
    pub token_budget: usize,
}

/// 更新 AI 运行时设置的部分请求。
///
/// 仅当字段被提供时才覆盖，便于只改其中一项；`api_key` 传空字符串表示显式清除。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAiSettings {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default)]
    pub token_budget: Option<usize>,
}

// ---------------------------------------------------------------------------
// 2.1 Research 多步研究（Phase 6）
// ---------------------------------------------------------------------------

/// 启动研究的入参。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResearchInput {
    pub question: String,
    /// 前端生成的本次运行 id，用于关联流式事件；缺省时后端生成（不推送）。
    #[serde(default)]
    pub run_id: Option<String>,
}

/// 一步工具调用（供 UI 展示研究过程）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStepDto {
    pub tool: String,
    pub args: serde_json::Value,
    pub summary: String,
}

/// 一次研究的诚实结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchReport {
    pub task_id: String,
    pub question: String,
    pub answer: String,
    /// 是否真的跑了 Agent 循环；false 时 `answer` 与 `steps` 为空。
    pub enabled: bool,
    pub note: Option<String>,
    pub steps: Vec<AgentStepDto>,
    /// Findings 对应的 Review 记录 id（结果只进 Review，不直接落库）。
    pub review_id: Option<String>,
}

// ---------------------------------------------------------------------------
// 2.2 Research 历史 / Timeline / Migration（Phase 4/6/8 收尾）
// ---------------------------------------------------------------------------

/// 研究任务历史卡片。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchTaskCard {
    pub id: String,
    pub question: String,
    pub status: String,
    /// findings.summary 的前缀（解析失败或缺失时为 `None`）。
    pub summary: Option<String>,
    pub created_at: String,
}

/// 时间轴上的一条事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    /// `document` | `claim` | `relation` | `research`。
    pub kind: String,
    pub id: String,
    pub title: String,
    pub detail: String,
    pub at: String,
}

/// 存量库探测结果（Phase 8）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationProbe {
    pub source_path: String,
    pub tables: Vec<String>,
    pub has_documents: bool,
    pub has_entities: bool,
    pub has_claims: bool,
    pub document_count: i64,
    pub claim_count: i64,
    pub compatible: bool,
}

/// 迁移报告（全部真实计数，跳过原因进 notes）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub source_path: String,
    pub backup_path: Option<String>,
    pub documents_imported: usize,
    pub documents_skipped: usize,
    pub claims_imported: usize,
    pub claims_skipped: usize,
    pub notes: Vec<String>,
}

/// 迁移入参。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationInput {
    pub source_path: String,
}
