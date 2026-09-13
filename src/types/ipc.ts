/**
 * IPC 契约类型定义 v1.0。
 *
 * 权威来源：`docs/IPC契约.md`。本文件必须与契约**逐字对应**：
 * 一个字段都不能少、不能改名、不能改类型。
 * 任何一侧（Rust / TS）改动都必须先改契约文档。
 *
 * 约定：
 * 1. 每个 command 只接收一个参数对象，固定命名为 `input`。
 * 2. 所有 DTO 使用 camelCase（Rust 侧 `#[serde(rename_all = "camelCase")]`）。
 * 3. 错误统一为 `{ code, message }`，前端只用 `code` 做逻辑判断。
 */

// ---------------------------------------------------------------------------
// 1.1 元信息
// ---------------------------------------------------------------------------

export interface AppInfo {
  name: string;
  version: string;
  dbPath: string;
  schemaVersion: number;
  /** 注册表内容指纹，见 docs/领域枚举与不变量定义.md §1.5 */
  registryVersion: string;
  /** 由 WIKIYA_API_KEY 环境变量决定；未配置 Key 时为 false */
  aiEnabled: boolean;
}

export interface Registries {
  entityTypes: { value: string; description: string }[];
  claimPredicates: string[];
  relationPredicates: {
    predicate: string;
    inverseLabel: string;
    symmetric: boolean;
    transitive: boolean;
    sourceTypes: string[];
    targetTypes: string[];
  }[];
  claimTypes: string[];
  polarities: string[];
  modalities: string[];
  claimStatuses: string[];
  entityStatuses: string[];
  relationStatuses: string[];
  ideaStatuses: string[];
  questionStatuses: string[];
  questionTypes: string[];
  eventTypes: string[];
  eventStatuses: string[];
  eventTimePrecisions: string[];
  researchTaskStatuses: string[];
  sourceTypes: string[];
  claimRelationTypes: string[];
  claimRelationStatuses: string[];
  normalizationOutcomes: string[];
  entityResolutionSteps: string[];
  loadStrategies: string[];
  agentRoles: string[];
  registryVersion: string;
}

export interface HealthReport {
  totalDocuments: number;
  totalChunks: number;
  totalEntities: number;
  totalClaims: number;
  totalEvidence: number;
  potentialDuplicates: number;
  unresolvedConflicts: number;
  claimsWithoutEvidence: number;
  unresolvedEntities: number;
  supersededClaims: number;
}

// ---------------------------------------------------------------------------
// 1.2 Document（Inbox）
// ---------------------------------------------------------------------------

export interface CreateDocumentInput {
  title: string;
  content: string;
  /** 默认 "note" */
  sourceType?: string;
  sourceUri?: string | null;
  metadata?: Record<string, unknown>;
}

export interface ListDocumentsInput {
  query?: string;
  limit?: number;
}

export interface GetDocumentInput {
  id: string;
}

export interface ReindexDocumentInput {
  id: string;
}

export interface DocumentSummary {
  id: string;
  title: string;
  sourceType: string;
  sourceUri: string | null;
  contentHash: string;
  chunkCount: number;
  charCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface DocumentDetail {
  document: DocumentSummary;
  content: string;
  chunks: ChunkCard[];
  claims: ClaimCard[];
}

export interface ChunkCard {
  id: string;
  documentId: string;
  chunkIndex: number;
  startOffset: number;
  endOffset: number;
  content: string;
  charCount: number;
}

// ---------------------------------------------------------------------------
// 1.3 Knowledge（Entity / Claim / Evidence）
// ---------------------------------------------------------------------------

export interface ListEntitiesInput {
  query?: string;
  entityType?: string;
  limit?: number;
}

export interface GetEntityInput {
  id: string;
  depth?: number;
}

export interface ListClaimsInput {
  subjectId?: string;
  predicate?: string;
  status?: string;
  documentId?: string;
  limit?: number;
}

export interface GetClaimInput {
  id: string;
}

export interface GetClaimHistoryInput {
  id: string;
}

export interface ListEvidenceInput {
  claimId: string;
}

export interface EntityCard {
  id: string;
  name: string;
  primaryType: string;
  types: string[];
  description: string | null;
  status: string;
  aliasCount: number;
  claimCount: number;
}

export interface EntityDetail {
  entity: EntityCard;
  aliases: string[];
  claims: ClaimCard[];
  relations: RelationCard[];
  graph: { nodes: GraphNode[]; edges: GraphEdge[] };
}

export interface RelationCard {
  id: string;
  sourceId: string;
  sourceName: string;
  predicate: string;
  inverseLabel: string | null;
  targetId: string;
  targetName: string;
  confidence: number | null;
  status: string;
}

export interface GraphNode {
  id: string;
  name: string;
  type: string;
  status: string;
  depth: number;
}

export interface GraphEdge {
  source: string;
  target: string;
  predicate: string;
}

export interface ClaimCard {
  id: string;
  subjectId: string;
  subjectName: string;
  predicate: string;
  objectId: string | null;
  objectName: string | null;
  objectText: string | null;
  content: string | null;
  claimType: string;
  polarity: string;
  modality: string;
  confidence: number | null;
  status: string;
  /** 来源文档可缺失（迁移导入、无文档来源的手工 Claim）→ 可空。 */
  sourceDocumentId: string | null;
  sourceDocumentTitle: string | null;
  sourceQuote: string | null;
  createdAt: string;
  /**
   * 派生字段，不落库（INV-04 in docs/领域枚举与不变量定义.md §4.7）。
   * `excluded` = 不参与检索（rejected / archived / draft），与「历史」不同。
   */
  lifecycle: 'current' | 'superseded' | 'excluded';
}

export interface ClaimDetail {
  claim: ClaimCard;
  evidence: EvidenceCard[];
  /** 与其他 claim 的演化关系（accepted 优先） */
  relations: ClaimRelationCard[];
  /** 该 claim 相关的全部关系，按时间倒序 */
  history: ClaimRelationCard[];
}

export interface EvidenceCard {
  id: string;
  documentId: string;
  /** 切片被删除 / 迁移导入时，以下字段可能为 null → 与后端 Option 对齐。 */
  documentTitle: string | null;
  chunkId: string | null;
  startOffset: number | null;
  endOffset: number | null;
  quote: string | null;
  /** 1..5 */
  evidenceLevel: number;
  /** quote / quote+context / paragraph / chunk / multi-chunk */
  evidenceLevelName: string;
}

/**
 * 手动录入路径：AI 关闭时的降级方案（对应分析报告 R2）。
 */
export interface CreateClaimInput {
  /** 实体名；不存在时按 canonical name 创建（status=candidate） */
  subject: string;
  /** 必须 ∈ claimPredicates，否则 DOMAIN_RULE_VIOLATION */
  predicate: string;
  object?: string | null;
  content?: string | null;
  claimType?: string;
  polarity?: string;
  modality?: string;
  confidence?: number | null;
  documentId: string;
  chunkId?: string | null;
  quote?: string | null;
  /** 默认 candidate */
  status?: string;
}

// ---------------------------------------------------------------------------
// 1.4 Search
// ---------------------------------------------------------------------------

export type SearchKind = 'document' | 'chunk' | 'claim' | 'entity';
export type SearchMethod = 'lexical' | 'semantic' | 'hybrid';

export interface SearchInput {
  query: string;
  limit?: number;
  semantic?: boolean;
  /** 取值：document | chunk | claim | entity；缺省为全部 */
  kinds?: string[];
}

export interface SearchResponse {
  query: string;
  tookMs: number;
  method: SearchMethod;
  total: number;
  hits: SearchHit[];
  /** 降级说明（如请求语义检索但仅有词法可用）。有值时必须如实展示给用户。 */
  notice?: string | null;
}

export interface SearchHit {
  kind: 'document' | 'chunk' | 'claim' | 'entity';
  id: string;
  title: string;
  snippet: string;
  score: number;
  method: string;
  /** 命中的字段名，用于解释「为什么这条匹配」 */
  matchedIn: string[];
  documentId?: string;
  claimId?: string;
}

// ---------------------------------------------------------------------------
// 1.5 Evolution / Review
// ---------------------------------------------------------------------------

export interface AnalyzeDocumentInput {
  documentId: string;
}

export interface ListReviewItemsInput {
  limit?: number;
}

export type ClaimRelationDecision = 'accept' | 'reject' | 'reset';

export interface DecideClaimRelationInput {
  relationId: string;
  decision: ClaimRelationDecision;
  /** 允许审核时修正关系类型 */
  relationship?: string;
}

export interface AnalysisReport {
  documentId: string;
  claimsScanned: number;
  relationsWritten: number;
  verdicts: ClaimRelationCard[];
}

export interface ClaimRelationCard {
  id: string;
  sourceClaimId: string;
  sourceText: string;
  targetClaimId: string;
  targetText: string;
  /** duplicate|coexists|supplements|supersedes|contradicts|unclear */
  relationship: string;
  /** candidate|accepted|rejected */
  status: string;
  confidence: number | null;
  reason: string | null;
  suggestedAction: string | null;
  targetPreviousStatus: string | null;
  createdAt: string;
}

export interface ReviewItem {
  relation: ClaimRelationCard;
  /** duplicate<supersedes<contradicts<coexists<unclear */
  priority: number;
  whatChanged: string;
  why: string;
  evidenceQuote: string | null;
  impact: string;
}

// ---------------------------------------------------------------------------
// 1.6 AI 抽取（Phase 5）
// ---------------------------------------------------------------------------

export interface ExtractClaimsInput {
  id: string;
}

export interface ExtractedClaim {
  subject: string;
  predicate: string;
  objectText?: string | null;
  content?: string | null;
  claimType?: string | null;
  polarity?: string | null;
  modality?: string | null;
  confidence?: number | null;
  sourceChunkIndex?: number | null;
  sourceQuote?: string | null;
  sentence?: string | null;
  /** 谓语是否通过受控词表校验；false 表示已被排除，不应落库 */
  accepted: boolean;
  rejectReason?: string | null;
}

export interface ExtractionReport {
  documentId: string;
  /** openai-compatible | offline */
  provider: string;
  enabled: boolean;
  note: string | null;
  extracted: ExtractedClaim[];
}

// ---------------------------------------------------------------------------
// 1.7 Ask 问答（Phase 6）
// ---------------------------------------------------------------------------

export interface AskRequest {
  question: string;
  /** 缺省 auto（后端选 KnowledgeAgent） */
  role?: string;
  /** 前端生成的本次运行 id，用于订阅 `agent-events` 流式事件 */
  runId?: string;
}

export interface AskSource {
  index: number;
  kind: string;
  id: string;
  title: string;
  snippet: string;
}

export interface ContextStats {
  totalTokens: number;
  loadedTokens: number;
  itemCount: number;
  truncated: boolean;
  /** 原始 token / 加载 token（无压缩时为 1） */
  compressionRatio: number;
}

export interface AskResponse {
  question: string;
  answer: string;
  enabled: boolean;
  note: string | null;
  sources: AskSource[];
  /** 上下文统计（预算 / 命中 / 截断），透明可审计 */
  contextStats: ContextStats | null;
}

// ---------------------------------------------------------------------------
// 2.0 AI 运行时设置（Phase 6 配置化）
// ---------------------------------------------------------------------------

export interface AiSettings {
  /** 是否已配置 API Key（不回显明文，出于安全） */
  apiKeySet: boolean;
  baseUrl: string;
  model: string;
  embeddingModel: string;
  /** Ask/Research 的上下文预算（token） */
  tokenBudget: number;
}

export interface UpdateAiSettings {
  /** 传空字符串表示显式清除；留空（undefined）表示不修改 */
  apiKey?: string | null;
  baseUrl?: string;
  model?: string;
  embeddingModel?: string;
  tokenBudget?: number;
}

// ---------------------------------------------------------------------------
// 2.1 Research 多步研究（Phase 6）
// ---------------------------------------------------------------------------

export interface AgentStep {
  tool: string;
  args: unknown;
  summary: string;
}

export interface ResearchReport {
  taskId: string;
  question: string;
  answer: string;
  /** 是否真的跑了 Agent 循环；false 时 answer 与 steps 为空 */
  enabled: boolean;
  note: string | null;
  steps: AgentStep[];
  /** Findings 对应的 Review 记录 id（结果只进 Review，不直接落库） */
  reviewId: string | null;
}

export interface StartResearchInput {
  question: string;
  /** 前端生成的本次运行 id，用于订阅 `agent-events` 流式事件 */
  runId?: string;
}

/** 研究任务历史卡片。 */
export interface ResearchTaskCard {
  id: string;
  question: string;
  status: string;
  summary: string | null;
  createdAt: string;
}

/** 时间轴上的一条事件。 */
export interface TimelineItem {
  kind: 'document' | 'claim' | 'relation' | 'research';
  id: string;
  title: string;
  detail: string;
  at: string;
}

/** 存量库探测结果（Phase 8）。 */
export interface MigrationProbe {
  sourcePath: string;
  tables: string[];
  hasDocuments: boolean;
  hasEntities: boolean;
  hasClaims: boolean;
  documentCount: number;
  claimCount: number;
  compatible: boolean;
}

/** 迁移报告（全部真实计数，跳过原因进 notes）。 */
export interface MigrationReport {
  sourcePath: string;
  backupPath: string | null;
  documentsImported: number;
  documentsSkipped: number;
  claimsImported: number;
  claimsSkipped: number;
  notes: string[];
}

export interface MigrationInput {
  sourcePath: string;
}

/** Agent 运行事件（TDD §53，与 Rust `AppEvent` 逐字段对齐，snake_case tag）。 */
export type AgentEvent =
  | { type: 'agent_started'; run_id: string; agent: string }
  | { type: 'agent_thinking'; run_id: string; text: string }
  | { type: 'token_delta'; run_id: string; delta: string }
  | { type: 'tool_called'; run_id: string; tool: string; arguments: unknown }
  | { type: 'tool_completed'; run_id: string; tool: string; ok: boolean; summary: string }
  | { type: 'agent_finished'; run_id: string; status: string };
