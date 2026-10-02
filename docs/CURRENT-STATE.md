# CURRENT-STATE.md — M0 现状盘点

> 日期：2026-10-02 ｜ 分支：main（8f88d8c）｜ Rig PoC：poc/rig-core 分支（42f9720，未合并）
>
> 本文只回答四个问题：**已经有什么？哪些是半成品？哪些是重复实现？哪些应该保留？**
> 后续所有阶段（M1-M13）的代码改动以本文为事实基线，不再凭感觉。

---

## 0. 事实澄清：Rig 集成状态（重要）

| 说法 | 事实 |
|---|---|
| "已集成 Rig" | ❌ 不成立。**main 的 Agent Runtime 是自研 ReAct 循环**（`src-tauri/src/ai/runtime.rs`，~310 行：JSON 动作协议、8 轮上限、12 工具白名单、流式、遥测）。 |
| Rig 的位置 | 仅在 `poc/rig-core` 分支的 PoC（`ai/provider_rig.rs`，feature `poc-rig` 门控，未合并）。它适配的是 **Provider trait**，不是 Agent Runtime 替换。 |
| PoC 确认的硬约束 | ① rig 0.42 默认走 OpenAI **Responses API**，兼容端点必须显式 `.completions_api()` 切回 `/chat/completions`；② rig-derive 用 let-chains，**要求 rustc ≥ 1.88 且上游不声明 MSRV**；③ rig 全异步，与同步 Provider trait / rusqlite 之间必须有桥接层。 |

**结论**：「M4 用 Rig AgentRunner 做执行底座」是**待验证的架构决策**，不是既成事实。M4 动工前需先验证 rig 的 AgentRunner/Hook 对同步工具与遥测落库的适配成本。

---

## 1. 能力 → 代码 → 数据库 → UI 映射（对照目标模型）

图例：✅ 完整 ｜ 🟡 半成品 ｜ ❌ 缺失

### Agent
- 代码：`ai/runtime.rs`（自研 ReAct 循环）、`ai/agents.rs`（7 角色：Auto/Personal/Knowledge/Research/Curator/Review/Extraction，硬编码系统提示）
- DB：`agent_runs`（运行记录）、`agent_events`（每步工具明细，UNIQUE(run_id, step_index)）
- UI：`AskPage`、`ResearchPage`（经 `agent-events` 频道实时展示步骤）
- 事件：`AppEvent` 6 个 Agent 变体（AgentStarted/Thinking/TokenDelta/ToolCalled/ToolCompleted/AgentFinished）→ `agent-events` 频道
- 状态：✅ 核心闭环完整；🟡 角色不可配置（M4）、无 Skill 层（M2）
- 保留：**是**。循环/白名单/遥测/事件四件套工作正常，M4 之前不需要重写。

### Skill
- 代码：**零**。全仓 13 处 "skill" 全部在文档与注释（0001 迁移注释里的 cache_key 组成描述与实际实现不符——实际 `context_cache_key` 只哈希 question+model+registry/schema+内容指纹）。
- DB：`skills` / `skill_versions` 表**从未创建**。
- 状态：❌ 纯纸面设计残留。M2 从零开始。

### Tool
- 代码：`ai/tools.rs` — 12 个白名单工具（search_knowledge / get_knowledge / get_entities / get_entity / get_claim / get_evidence / find_related / compare_claims / detect_conflict / propose_evolution / request_review / research[未启用]）。全部同步，直接持 `&Connection`；**禁止 execute_sql**；唯一写路径 `request_review`（只写 reviews 表）。
- 状态：✅ 完整。M2/M5 的权限模型（READ/PROPOSE）可直接映射到现有白名单。

### Run
- 代码：两套并存 —— ① Extraction Run（`extraction_service.rs` + `extraction_runs` 表，status 6 值/stage 6 值/中断恢复/取消）；② Agent Run（`telemetry_repository::start/finish_agent_run` + `agent_runs` 表，仅为遥测，无状态机、无取消、无恢复）。
- 状态：🟡 **正是 M1 要统一的割裂点**。Ask Run / Skill Run / Review Run 不存在。

### Event
- 代码：`events/app_event.rs` — `AppEvent` 12 变体（Agent 6 + 知识 6）→ `agent-events` 频道（前端带 runId 才启用）；`events/extraction_event.rs` — `ExtractionEvent` 8 变体 → `extraction-events` 频道（始终启用）。
- 状态：🟡 两个频道、两套枚举、两种启用语义。M1 统一为单一 RunEvent。

### Extraction
- 代码：`ai_service.rs`（同步整篇 + `batch_chunks` 按块数/字符双预算分批）+ `extraction_service.rs`（后台管线 Preparing→Chunking→Extracting→Validating→Comparing→Finalizing，spawn_blocking 承载同步调用）。
- 可靠性：`Modality::canonical` 系词别名；`parse_claims` 截断抢救（`salvage_objects` 状态机配平恢复完整对象）。
- UI：`ExtractionPanel`（run-based 实时进度 + resume）、`ActivityPanel`（运行历史）、`HomePage`（捕获后 run-based 抽取）。
- 状态：✅ 当前最完整的能力。M6 迁移为 Skill 时管线可整体复用。

### Claim / Evidence / Entity
- 代码：`knowledge_service.rs`（读取+手动录入，与 AI 抽取同套受控词表校验）；`domain/knowledge/`（ClaimPredicate/Modality 受控词表）。
- DB：`entities`/`entity_aliases`/`claims`（含 observed_at）/`evidence`/`claim_relations`/`relations`。
- UI：`KnowledgePage`、`ClaimDetailPage`、`DocumentDetailPage`、`CreateClaimDialog`。
- 状态：✅ 领域核心稳固，**保留不动**。

### Evolution / Review
- 代码：`evolution_service.rs`（确定性分析，不调 LLM；analyze/decide/rollback/accept_existing）+ `domain/evolution/engine`、`decision.rs`/`resolver.rs`；`review_service.rs`（队列读取 + 用户决策）。
- DB：`claim_relations`（6 关系×3 状态）、`claim_relation_events`（0004 只追加历史）、`reviews`（多态 target，含 agent_proposal）。
- UI：`ReviewPage`、`TimelinePage`。
- 状态：✅ 完整。M10 Correction Skill 直接复用，**不需要新建系统**。

### Trace（Provenance）
- 代码：🟡 碎片化 —— `agent_runs/agent_events`（Agent 步骤）、`claim_relation_events`（演化日志）、`extraction_runs.result_json`（抽取结果）、`context_runs/context_sections`（上下文编译）、`evidence`（知识溯源）。**彼此无外键关联、无统一 run_id 贯穿**。
- UI：❌ 无 Trace 页面（ClaimDetail 有历史标签）。
- 状态：**M1 的全部理由**。数据大多已存在，缺的是统一模型与关联。

### Ask / Search
- 代码：`ask_service.rs`（role 路由→检索→context 编排→流式带引用→遥测）；`search_service.rs`（多路召回+RRF）；`retrieval_service.rs`；`ai/context/`（纯函数引擎：plan/compress/compile/预算装桶）。
- DB：`chunk_embeddings`（0002）、`context_cache`/`context_runs`/`context_sections`/`task_packets`。
- UI：`SearchPage`（Search/Ask 统一入口）、`AskPage`（流式+引用+Context Stats）。
- 状态：✅ 完整。M9 只需补 Answer↔Run↔Evidence 关联。

### 其它完整能力
- Capture：`capture_service`（存原文+确定性切分，content_hash 幂等 INV-02，FTS5 trigram 触发器）✅
- Migration：`migration_service`（Backup→Probe→Import）+ `MigrationPage` ✅
- Settings：`settings_service` + `settings` 表（API Key AES-256-GCM 密文 `ai.api_key.enc`，`infrastructure/secrets.rs`）+ `SettingsPage` ✅
- MCP：`src-tauri/src/bin/mcp.rs`（独立二进制，未在 31 个命令内）🟡
- Provider：`ai/provider.rs`（OpenAiProvider 空内容升级重试 8 组合、reasoning_content 捞 JSON、流式兜底；RigProvider 为 PoC）✅

### 命令与 API
- 31 个 Tauri 命令（`lib.rs:118-161` 注册）↔ `src/lib/api.ts` 31 个函数一一对应，统一 `call()` 包装与 `WikiError` 7 错误码。

---

## 2. 半成品清单

1. **Run 双轨制**：Extraction Run 有状态机+取消+恢复；Agent Run 只是遥测行。M1 统一。
2. **Event 双频道双枚举**：`agent-events`（条件启用）vs `extraction-events`（恒启用），启用语义不一致。
3. **Skill**：文档 7 类版本化对象（prompt/skill/reference/ontology/schema/history/task_packet）里，只有 registry/schema 真正进入 cache key。
4. **research 工具**：白名单里注册但诚实报错未启用（`tools.rs:154`）。
5. **MCP**：二进制存在但无文档、无 UI 入口、未纳入命令体系。
6. **`extract_claims` 同步命令**：功能已被 Extraction Run 取代，前端 31 个 api 里它已无调用方——可删除的迁移残留。
7. **`evolution_service::decide_relation/rollback`** 与 `review_service::decide_relation` 存在职责重叠（命令层只走后者）。

## 3. 重复实现 / 割裂点

| 割裂 | 表现 | M 落点 |
|---|---|---|
| Run 概念 | extraction_runs vs agent_runs 两套 schema、两种生命周期 | M1 |
| 事件 | 两频道两枚举，前端两个 hook | M1 |
| 进度 | Research 有步骤事件无 Run 行；Ask 有遥测无事件透出 | M1 |
| Prompt 来源 | agents.rs 硬编码 7 段提示词 vs 规划的 prompt_profiles | M3 |
| 决策入口 | review_service vs evolution_service 的 decide 重叠 | M1 盘点后合并 |
| 历史视图 | get_claim_history、claim_relation_events、agent_events 三处各看各的 | M8 |

## 4. 应该保留的（不推倒重来）

- **领域层**（domain/：Ontology 受控词表、Claim/Evidence/Evolution 不变量、INV 约束）—— 纯净且测试覆盖 310 用例。
- **Review 闸门**：「AI suggests, user decides」已由 reviews 表 + 唯一写路径结构性保证。
- **Extraction Run 管线**：状态机/取消/恢复/分批/截断抢救全套，M6 打包成 Skill 时整体复用。
- **Context Efficiency Engine**：纯函数、可测试，M9 直接续用。
- **Provider 抽象 + 重试策略**：兼容性实战经验（空 content、reasoning_content、流式兜底）值得保留；rig PoC 证明了替换的成本与约束。
- **DTO 单形状约定**（dto.rs）与 31 命令↔31 api 的一一对应。

## 5. M1（统一 Trace）的最小落点建议

基于现状，M1 的第一步不是建新表，而是：

1. **统一 run 表**：新增 `runs`（id/parent_run_id/type/actor/status/started_at/finished_at/metadata），把 `extraction_runs` 与 `agent_runs` 迁移为 `type` 区分的行（或加 run_id 列做桥接，避免大迁移）。
2. **统一事件枚举**：`RunEvent`（started/stage_changed/tool_called/tool_completed/candidate_created/proposal_created/completed/failed/cancelled），单频道 `run-events`；旧频道保留一个过渡期。
3. **贯穿 ID**：candidate/evidence/review/answer 行增加 `run_id` 列，让「Source→Extraction Run→Skill→Candidate→Evidence→Evolution→Review→Current」链条可以先靠 SQL JOIN 跑通（Trace UI 是 M8，先修数据模型）。

---

*M0 完成。下一步：M1 统一 Trace。*
