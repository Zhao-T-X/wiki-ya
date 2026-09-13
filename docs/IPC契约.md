# wiki-ya

## IPC 契约 v1.0（前端 ↔ Rust）

> 这是前端与后端的**唯一接口约定**。任何一侧改动都必须先改本文件。
> 实现位置：`src-tauri/src/commands/**`（Rust）与 `src/lib/api.ts`（TS）。

---

## 0. 约定

1. **每个 command 只接收一个参数对象，固定命名为 `input`**，避免 Tauri 参数名大小写转换的歧义。
   - **无参 command 也必须传 `input`**：前端统一 `invoke(cmd, { input })`，无参时传 `{}`。
     这不是冗余，因为 Tauri 会把 Rust 参数名当作 args 的键，键缺失会直接报错。
   - 对应实现：Rust 侧无参 command 声明为 `input: Option<serde_json::Value>` 并忽略它。
2. 所有 DTO 使用 `#[serde(rename_all = "camelCase")]`，因此 TS 侧是天然 camelCase。
3. **DTO 的枚举字段用 `String` 而不是 Rust 枚举**：领域枚举的反序列化会拒绝未知值，
   一旦库里存在历史遗留值，整个接口会失败。合法性在**写入**路径上严格把关，
   读取路径则宽容透传，让 UI 有机会把问题展示出来。
4. 错误统一为 `{ code, message }`。**前端不得解析 `message` 做逻辑判断**，只用 `code`：

| code | 含义 | UI 处理 |
|---|---|---|
| `NOT_FOUND` | 目标不存在 | 显示空态 |
| `INVALID_INPUT` | 入参校验失败 | 表单内联提示 |
| `CONFLICT` | 幂等键冲突（如重复导入） | 提示"已存在"并跳转 |
| `DOMAIN_RULE_VIOLATION` | 违反受控词表/领域不变量 | 明确报错，不降级 |
| `DATABASE_ERROR` | 数据库故障 | 全局提示 |
| `INTERNAL_ERROR` | 其他 | 全局提示 + 日志 |

4. `invoke` 包装函数一律 `async`，错误在 `src/lib/api.ts` 中统一转成 `WikiError`。

---

## 1. Commands

### 1.1 元信息

```ts
app_info(input: Record<string, never>): AppInfo
list_registries(input: Record<string, never>): Registries
knowledge_health(input: Record<string, never>): HealthReport
```

```ts
interface AppInfo {
  name: string;
  version: string;
  dbPath: string;
  schemaVersion: number;
  registryVersion: string;   // 注册表内容指纹，见 docs/领域枚举与不变量定义.md §1.5
  aiEnabled: boolean;        // 由 WIKIYA_API_KEY 环境变量决定；未配置 Key 时为 false
}

interface Registries {
  entityTypes: { value: string; description: string }[];
  claimPredicates: string[];
  relationPredicates: {
    predicate: string; inverseLabel: string; symmetric: boolean; transitive: boolean;
    sourceTypes: string[]; targetTypes: string[];
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

interface HealthReport {
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
```

### 1.2 Document（Inbox）

```ts
create_document(input: {
  title: string;
  content: string;
  sourceType?: string;              // 默认 "note"
  sourceUri?: string | null;
  metadata?: Record<string, unknown>;
}): DocumentSummary
```

- 同步完成：写 `documents` + 切分 `chunks`（**不调用 LLM**，Local-first 无 Key 也能用）。
- 相同 `content`（SHA-256）已存在 → `CONFLICT`，`message` 中不带 id，前端应调用 `list_documents({ query })` 定位。
- 同一事务内完成 doc + chunks（INV-19）。

```ts
list_documents(input: { query?: string; limit?: number }): DocumentSummary[]
get_document(input: { id: string }): DocumentDetail
reindex_document(input: { id: string }): DocumentSummary   // 原文不变，仅重建 chunks
```

```ts
interface DocumentSummary {
  id: string; title: string; sourceType: string; sourceUri: string | null;
  contentHash: string; chunkCount: number; charCount: number;
  createdAt: string; updatedAt: string;
}

interface DocumentDetail {
  document: DocumentSummary;
  content: string;
  chunks: ChunkCard[];
  claims: ClaimCard[];
}

interface ChunkCard {
  id: string; documentId: string; chunkIndex: number;
  startOffset: number; endOffset: number; content: string; charCount: number;
}
```

### 1.3 Knowledge（Entity / Claim / Evidence）

```ts
list_entities(input: { query?: string; entityType?: string; limit?: number }): EntityCard[]
get_entity(input: { id: string; depth?: number }): EntityDetail
list_claims(input: {
  subjectId?: string; predicate?: string; status?: string; documentId?: string; limit?: number;
}): ClaimCard[]
get_claim(input: { id: string }): ClaimDetail
get_claim_history(input: { id: string }): ClaimRelationCard[]
create_claim(input: CreateClaimInput): ClaimCard
list_evidence(input: { claimId: string }): EvidenceCard[]
```

```ts
interface EntityCard {
  id: string; name: string; primaryType: string; types: string[];
  description: string | null; status: string;
  aliasCount: number; claimCount: number;
}

interface EntityDetail {
  entity: EntityCard;
  aliases: string[];
  claims: ClaimCard[];
  relations: RelationCard[];
  graph: { nodes: GraphNode[]; edges: GraphEdge[] };
}

interface RelationCard {
  id: string; sourceId: string; sourceName: string;
  predicate: string; inverseLabel: string | null;
  targetId: string; targetName: string;
  confidence: number | null; status: string;
}

interface GraphNode { id: string; name: string; type: string; status: string; depth: number }
interface GraphEdge { source: string; target: string; predicate: string }

interface ClaimCard {
  id: string;
  subjectId: string; subjectName: string;
  predicate: string;
  objectId: string | null; objectName: string | null; objectText: string | null;
  content: string | null;
  claimType: string; polarity: string; modality: string;
  confidence: number | null; status: string;
  sourceDocumentId: string; sourceDocumentTitle: string;
  sourceQuote: string | null;
  createdAt: string;
  lifecycle: 'current' | 'superseded';   // 派生字段，不落库（INV-04 in docs/领域枚举与不变量定义.md §4.7）
}

interface ClaimDetail {
  claim: ClaimCard;
  evidence: EvidenceCard[];
  relations: ClaimRelationCard[];   // 与其他 claim 的演化关系（accepted 优先）
  history: ClaimRelationCard[];     // 该 claim 相关的全部关系，按时间倒序
}

interface EvidenceCard {
  id: string;
  documentId: string; documentTitle: string;
  chunkId: string;
  startOffset: number; endOffset: number;
  quote: string | null;
  evidenceLevel: number;                   // 1..5
  evidenceLevelName: string;               // quote / quote+context / paragraph / chunk / multi-chunk
}

// 手动录入路径：AI 关闭时的降级方案（对应分析报告 R2）
interface CreateClaimInput {
  subject: string;                  // 实体名；不存在时按 canonical name 创建（status=candidate）
  predicate: string;                // 必须 ∈ claimPredicates，否则 DOMAIN_RULE_VIOLATION
  object?: string | null;
  content?: string | null;
  claimType?: string; polarity?: string; modality?: string;
  confidence?: number | null;
  documentId: string;
  chunkId?: string | null;
  quote?: string | null;
  status?: string;                  // 默认 candidate
}
```

### 1.4 Search

```ts
search(input: { query: string; limit?: number; semantic?: boolean; kinds?: string[] }): SearchResponse
```

- `semantic` 在 Phase 6 前恒为降级：只做 `lexical`（FTS5 trigram）。请求 `semantic: true` 时返回 `method: "lexical"`，**不报错**。
- `kinds` 取值：`document | chunk | claim | entity`；缺省为全部。

```ts
interface SearchResponse {
  query: string; tookMs: number; method: 'lexical' | 'semantic' | 'hybrid';
  total: number; hits: SearchHit[];
}

interface SearchHit {
  kind: 'document' | 'chunk' | 'claim' | 'entity';
  id: string; title: string; snippet: string;
  score: number; method: string;
  matchedIn: string[];               // 命中的字段名，用于解释「为什么这条匹配」
  documentId?: string; claimId?: string;
}
```

### 1.5 Evolution / Review

```ts
analyze_document(input: { documentId: string }): AnalysisReport
list_review_items(input: { limit?: number }): ReviewItem[]
decide_claim_relation(input: {
  relationId: string;
  decision: 'accept' | 'reject' | 'reset';
  relationship?: string;             // 允许审核时修正关系类型
}): ClaimRelationCard
```

- `analyze_document` 是**纯确定性**的（`docs/领域枚举与不变量定义.md` §4.4），不调 LLM。
- `decide_claim_relation` 是 `superseded` 状态的**唯一入口**（INV-08/INV-09）：
  - `accept` + `supersedes` → 记录 `targetPreviousStatus`，旧 claim 置 `superseded`
  - `reset` / 从已确认状态回退 → 精确恢复 `targetPreviousStatus`
  - `reject` → 关系置 `rejected`，**不改动任何 claim**

```ts
interface AnalysisReport {
  documentId: string;
  claimsScanned: number;
  relationsWritten: number;
  verdicts: ClaimRelationCard[];
}

interface ClaimRelationCard {
  id: string;
  sourceClaimId: string; sourceText: string;
  targetClaimId: string; targetText: string;
  relationship: string;              // duplicate|coexists|supplements|supersedes|contradicts|unclear
  status: string;                    // candidate|accepted|rejected
  confidence: number | null;
  reason: string | null;
  suggestedAction: string | null;
  targetPreviousStatus: string | null;
  createdAt: string;
}

interface ReviewItem {
  relation: ClaimRelationCard;
  priority: number;                  // duplicate<supersedes<contradicts<coexists<unclear
  whatChanged: string;
  why: string;
  evidenceQuote: string | null;
  impact: string;
}
```

---

## 1.6 AI 抽取（Phase 5）

```ts
extract_claims(input: { id: string }): ExtractionReport
```

- 从一篇文档抽取 Claim 候选。**不落库**：返回的 `extracted` 只是预览，逐条接受时
  由前端调用 `create_claim` + `analyze_document`（复用既有 Review 流程），由用户最终决定。
- `enabled: false` 时 `extracted` 必为空，`note` 说明原因（多为未配置 `WIKIYA_API_KEY`），
  UI 必须如实展示「AI 未启用」，**绝不伪造**任何抽取结果（PRD「AI suggests, user decides」）。
- 每条 `ExtractedClaim` 的 `accepted` 表示谓语是否通过了受控词表校验；
  `accepted: false` 的条目已被排除，不应进入落库流程。

```ts
interface ExtractionReport {
  documentId: string;
  provider: string;            // openai-compatible | offline
  enabled: boolean;
  note: string | null;
  extracted: ExtractedClaim[];
}

interface ExtractedClaim {
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
  accepted: boolean;
  rejectReason?: string | null;
}
```

---

## 1.7 Ask 问答（Phase 6）

```ts
ask(input: { question: string; role?: string }): AskResponse
```

- 基于本地知识库问答，回答带编号引用 `[n]`（与 `sources` 的 `index` 对齐），UI 可逐级下钻
  Source → Chunk → Claim（TDD §65）。
- `enabled: false` 时 `answer` 与 `sources` 必为空，`note` 说明原因（多为未配置 `WIKIYA_API_KEY`），
  UI 必须如实展示「AI 未启用」，绝不伪造答案或引用（PRD「AI suggests, user decides」）。
- `contextStats` 透明展示本次上下文预算、实际加载 token、命中条数与是否截断，便于审计。
- `sources` 只包含实际进入模型上下文的条目；检索不到相关段落时模型被要求明确说「未找到」。

```ts
interface AskRequest {
  question: string;
  role?: string;            // 缺省 auto（后端选 KnowledgeAgent）
}

interface AskSource {
  index: number;            // 与回答中的 [n] 对齐
  kind: string;             // document | chunk | claim | entity | evidence
  id: string;
  title: string;
  snippet: string;
}

interface ContextStats {
  totalTokens: number;
  loadedTokens: number;
  itemCount: number;
  truncated: boolean;
  compressionRatio: number;
}

interface AskResponse {
  question: string;
  answer: string;
  enabled: boolean;
  note: string | null;
  sources: AskSource[];
  contextStats: ContextStats | null;
}
```

---

## 2. 前端类型来源

`src/types/ipc.ts` 必须与本文档逐字对应；`src/lib/api.ts` 只做 `invoke` + 错误包装，
**不得**在 API 层做业务判断或字段改名。
