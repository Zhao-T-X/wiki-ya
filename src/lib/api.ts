/**
 * `invoke` 封装层。
 *
 * 职责边界（IPC 契约 §2）：只做 `invoke` + 错误包装，
 * **不得**在 API 层做业务判断或字段改名。
 *
 * 额外职责：检测非 Tauri 环境（例如 `pnpm dev` 的浏览器预览）。
 * 在这种环境下不抛运行时异常、不崩页面，而是让所有调用返回可识别的
 * `WikiError('NOT_IN_TAURI')`，由 UI 展示提示条，从而支持单独预览界面。
 */

import { invoke } from '@tauri-apps/api/core';

import type {
  AnalysisReport,
  AppInfo,
  ClaimCard,
  ClaimDetail,
  ClaimRelationCard,
  CreateClaimInput,
  CreateDocumentInput,
  DecideClaimRelationInput,
  DocumentDetail,
  DocumentSummary,
  EntityCard,
  EntityDetail,
  EvidenceCard,
  ExtractClaimsInput,
  ExtractionReport,
  ExtractionRunDto,
  AskRequest,
  AskResponse,
  AiSettings,
  UpdateAiSettings,
  ResearchReport,
  ResearchTaskCard,
  StartResearchInput,
  TimelineItem,
  MigrationInput,
  MigrationProbe,
  MigrationReport,
  GetDocumentInput,
  GetEntityInput,
  HealthReport,
  ListClaimsInput,
  ListDocumentsInput,
  ListEntitiesInput,
  ListEvidenceInput,
  Registries,
  ReviewItem,
  SearchInput,
  SearchResponse,
} from '@/types/ipc';

/** 错误码：契约定义的 6 个 + 前端本地新增的 NOT_IN_TAURI。 */
export type WikiErrorCode =
  | 'NOT_FOUND'
  | 'INVALID_INPUT'
  | 'CONFLICT'
  | 'DOMAIN_RULE_VIOLATION'
  | 'DATABASE_ERROR'
  | 'INTERNAL_ERROR'
  | 'NOT_IN_TAURI';

/** 统一错误类型：`code` 供逻辑判断，`message` 仅供展示。 */
export class WikiError extends Error {
  readonly code: WikiErrorCode;

  constructor(code: WikiErrorCode, message: string) {
    super(message);
    this.name = 'WikiError';
    this.code = code;
  }
}

/**
 * 是否运行在 Tauri WebView 内。
 * 浏览器预览时 `__TAURI_INTERNALS__` 不存在。
 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

const NOT_IN_TAURI_MESSAGE =
  '当前运行在浏览器预览模式，数据库不可用。请使用 `pnpm tauri:dev` 启动桌面应用后重试。';

function toWikiError(error: unknown): WikiError {
  if (error instanceof WikiError) return error;

  if (typeof error === 'object' && error !== null) {
    const record = error as Record<string, unknown>;
    const code = typeof record.code === 'string' ? record.code : 'INTERNAL_ERROR';
    const message =
      typeof record.message === 'string' ? record.message : JSON.stringify(record);
    return new WikiError(code as WikiErrorCode, message);
  }

  if (typeof error === 'string') {
    return new WikiError('INTERNAL_ERROR', error);
  }

  return new WikiError('INTERNAL_ERROR', '发生未知错误');
}

async function call<T>(command: string, input: unknown): Promise<T> {
  if (!isTauri()) {
    throw new WikiError('NOT_IN_TAURI', NOT_IN_TAURI_MESSAGE);
  }

  try {
    // 契约 §0：每个 command 只接收一个参数对象，固定命名为 input。
    return await invoke<T>(command, { input });
  } catch (error) {
    throw toWikiError(error);
  }
}

// ---------------------------------------------------------------------------
// 1.1 元信息
// ---------------------------------------------------------------------------

export function app_info(): Promise<AppInfo> {
  return call<AppInfo>('app_info', {});
}

export function list_registries(): Promise<Registries> {
  return call<Registries>('list_registries', {});
}

export function knowledge_health(): Promise<HealthReport> {
  return call<HealthReport>('knowledge_health', {});
}

// ---------------------------------------------------------------------------
// 1.1.1 AI 运行时设置（Phase 6 配置化）
// ---------------------------------------------------------------------------

export function get_settings(): Promise<AiSettings> {
  return call<AiSettings>('get_settings', {});
}

export function update_settings(input: UpdateAiSettings): Promise<AiSettings> {
  return call<AiSettings>('update_settings', input);
}

// ---------------------------------------------------------------------------
// 1.1.2 Research 多步研究（Phase 6）
// ---------------------------------------------------------------------------

export function start_research(input: StartResearchInput): Promise<ResearchReport> {
  return call<ResearchReport>('start_research', input);
}

export function list_research_tasks(): Promise<ResearchTaskCard[]> {
  return call<ResearchTaskCard[]>('list_research_tasks', {});
}

// ---------------------------------------------------------------------------
// 1.1.3 Timeline（Phase 4 收尾）
// ---------------------------------------------------------------------------

export function list_timeline(): Promise<TimelineItem[]> {
  return call<TimelineItem[]>('list_timeline', {});
}

// ---------------------------------------------------------------------------
// 1.1.4 存量库迁移（Phase 8）
// ---------------------------------------------------------------------------

export function probe_migration(input: MigrationInput): Promise<MigrationProbe> {
  return call<MigrationProbe>('probe_migration', input);
}

export function run_migration(input: MigrationInput): Promise<MigrationReport> {
  return call<MigrationReport>('run_migration', input);
}

// ---------------------------------------------------------------------------
// 1.2 Document（Inbox）
// ---------------------------------------------------------------------------

export function create_document(input: CreateDocumentInput): Promise<DocumentSummary> {
  return call<DocumentSummary>('create_document', input);
}

export function list_documents(input: ListDocumentsInput = {}): Promise<DocumentSummary[]> {
  return call<DocumentSummary[]>('list_documents', input);
}

export function get_document(input: GetDocumentInput): Promise<DocumentDetail> {
  return call<DocumentDetail>('get_document', input);
}

export function reindex_document(input: GetDocumentInput): Promise<DocumentSummary> {
  return call<DocumentSummary>('reindex_document', input);
}

// ---------------------------------------------------------------------------
// 1.3 Knowledge（Entity / Claim / Evidence）
// ---------------------------------------------------------------------------

export function list_entities(input: ListEntitiesInput = {}): Promise<EntityCard[]> {
  return call<EntityCard[]>('list_entities', input);
}

export function get_entity(input: GetEntityInput): Promise<EntityDetail> {
  return call<EntityDetail>('get_entity', input);
}

export function list_claims(input: ListClaimsInput = {}): Promise<ClaimCard[]> {
  return call<ClaimCard[]>('list_claims', input);
}

export function get_claim(input: { id: string }): Promise<ClaimDetail> {
  return call<ClaimDetail>('get_claim', input);
}

export function get_claim_history(input: { id: string }): Promise<ClaimRelationCard[]> {
  return call<ClaimRelationCard[]>('get_claim_history', input);
}

export function create_claim(input: CreateClaimInput): Promise<ClaimCard> {
  return call<ClaimCard>('create_claim', input);
}

export function list_evidence(input: ListEvidenceInput): Promise<EvidenceCard[]> {
  return call<EvidenceCard[]>('list_evidence', input);
}

// ---------------------------------------------------------------------------
// 1.4 Search
// ---------------------------------------------------------------------------

export function search(input: SearchInput): Promise<SearchResponse> {
  return call<SearchResponse>('search', input);
}

// ---------------------------------------------------------------------------
// 1.5 Evolution / Review
// ---------------------------------------------------------------------------

export function analyze_document(input: { documentId: string }): Promise<AnalysisReport> {
  return call<AnalysisReport>('analyze_document', input);
}

export function list_review_items(input: { limit?: number } = {}): Promise<ReviewItem[]> {
  return call<ReviewItem[]>('list_review_items', input);
}

export function decide_claim_relation(
  input: DecideClaimRelationInput,
): Promise<ClaimRelationCard> {
  return call<ClaimRelationCard>('decide_claim_relation', input);
}

// ---------------------------------------------------------------------------
// 1.6 AI 抽取（Phase 5）
// ---------------------------------------------------------------------------

export function extract_claims(input: ExtractClaimsInput): Promise<ExtractionReport> {
  return call<ExtractionReport>('extract_claims', input);
}

// ---------------------------------------------------------------------------
// 1.6.1 Extraction Run（EXTRACTION-001：异步抽取后台任务）
// ---------------------------------------------------------------------------

export function start_extraction(input: { id: string }): Promise<string> {
  return call<string>('start_extraction', input);
}

export function get_extraction_run(input: { id: string }): Promise<ExtractionRunDto> {
  return call<ExtractionRunDto>('get_extraction_run', input);
}

export function list_extraction_runs(input?: { limit?: number }): Promise<ExtractionRunDto[]> {
  return call<ExtractionRunDto[]>('list_extraction_runs', input ?? {});
}

export function cancel_extraction(input: { id: string }): Promise<boolean> {
  return call<boolean>('cancel_extraction', input);
}

// ---------------------------------------------------------------------------
// 1.7 Ask 问答（Phase 6）
// ---------------------------------------------------------------------------

export function ask(input: AskRequest): Promise<AskResponse> {
  return call<AskResponse>('ask', input);
}
