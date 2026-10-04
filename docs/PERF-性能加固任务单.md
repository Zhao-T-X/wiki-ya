# 性能加固任务单（PERF-01 ~ PERF-06）

> 状态：**待执行**。本单基于对当前代码的逐条核对（不是推测），已标注修正项。
> 目标：把 wiki-ya 从「1000 chunk 好用」推到「10 万 chunk 可用」。

## 0. 核对结论：审计哪些属实

已逐条验证代码，**审计的核心判断全部成立**：

| 审计结论 | 代码证据 | 判定 |
| --- | --- | --- |
| 每次语义检索全库重新 embedding | `embedding_repository::all_chunk_texts` = `SELECT id, content FROM chunks`，无任何「已存在/是否过期」过滤 | 属实 |
| 暴力检索 + 全量排序 + 全量正文进内存 | `nearest_chunks` 把每行 `(score, RetrievedPassage{content})` 全塞进 `scored`，`sort_by` 后才 `take(top_k)` | 属实 |
| `candidates.accepted_claim_id` 无索引 | 仅 `idx_candidates_run` / `_status` / `_support_level`（0011 新增）；而 `get_claim_trace` 正是按该列查 | 属实 |
| `runs.parent_run_id` 无索引 | 仅 `idx_runs_started` / `idx_runs_type` | 属实 |
| `evidence` 缺复合索引 | 仅 `idx_evidence_claim` / `_document` / `_chunk` 三个单列 | 属实 |
| `idx_chunk_embeddings_model` 建了但查询不用 | 索引存在（0002），`nearest_chunks` 未按 model 过滤 | 属实 |
| 每个 IPC 重开 SQLite 连接 | `AppState { db_path }` + `state.open()` → `Connection::open` + 4 条 PRAGMA | 属实 |
| `journal_mode=WAL` 每次连接都设 | 在 `open()` 内 | 属实 |
| `reqwest::blocking::Client::new()` 每次调用新建 | `provider.rs` 三处 | 属实 |
| Rig 每次调用新建 Tokio runtime | `block_on` 内 `new_current_thread()` | 属实（P2） |
| 抽取过程内存三份副本 | `all: Vec<ExtractedClaim>` + DB candidate + `result_json` | 属实 |

### 需要修正 / 补充的地方

1. **`content_hash` 指纹现在不需要**（对审计 PERF-10 的前置建议）。
   `document_repository::replace_chunks` 是 `DELETE FROM chunks WHERE document_id=?` 后
   **以全新 id 插入**，因此「内容变 ⇒ chunk id 变 ⇒ 该 id 没有向量行」。
   只要 PERF-01 的判断条件是「当前模型下缺向量」，就天然覆盖内容变更。
   **但**：若将来实施 PERF-10（稳定 id 的增量 reindex），该不变量即失效，
   届时必须同时补 `chunks.content_hash` + 向量指纹，否则会读到过期向量。
   → 已写入 PERF-10 的前置条件。
2. **`evidence(claim_id, evidence_level, created_at)` 只是候选优化**，不是瓶颈。
   现有 `idx_evidence_claim` 已能定位到行，窗口函数额外成本取决于证据条数，
   优先级排在 `accepted_claim_id` / `parent_run_id` 之后。
3. **「合并 extraction_runs 与 runs」审计已自我否决，本单采纳**：
   两表职责不同，改为批量化写入即可（见 PERF-05）。
4. **Trace 改为按需下钻**（审计 P2-18）与 PR-01 已实现的「逐层下钻」同向，
   归入 PERF-06，不单列。

---

## PERF-01 Retrieval / Embedding Cache  P0  ✅ 已完成

**一句话**：语义检索只向量化「当前模型下还没有向量」的 chunk，且必须分批。

### 修改文件
- `src-tauri/src/infrastructure/embedding_repository.rs`（新增查询；`all_chunk_texts` 标 deprecated）
- `src-tauri/src/application/retrieval_service.rs`（`semantic_search_with_usage`）
- `src-tauri/src/ai/provider.rs`（若需要，按 char/token 预算切批）

### SQL
```sql
-- 只取「当前 embedding 模型下尚无向量」的 chunk
SELECT c.id, c.content
FROM chunks c
LEFT JOIN chunk_embeddings e
       ON e.chunk_id = c.id AND e.model = ?1
WHERE e.chunk_id IS NULL;
```
对应新增：
```rust
pub fn chunks_missing_embedding(conn: &Connection, model: &str)
    -> AppResult<Vec<(String, String)>>;
```

### 算法
1. `semantic_search_with_usage` 用 `chunks_missing_embedding` 取代 `all_chunk_texts`。
2. 命中为空 → **完全跳过向量化**（稳态零成本），直接用已有向量做检索。
3. 命中非空 → **按预算切批**调用 `provider.embed`，逐批 `store_embedding`
   （`chunk_id` 主键幂等，中途失败可安全重跑）。切批预算复用
   `ai_service::batch_chunks` 的「块数 + 字符双预算」思路，抽成通用函数。
4. 每次 `embed` 返回的 `prompt_tokens` 累加进 `TokenUsage.embedding_tokens`（保持账本诚实）。

### 验收指标
- 稳态（无新增 chunk）连续 3 次 Ask：`chunks_missing_embedding` 返回 0 行；
  `provider.embed` 仅被调用 1 次且入参为 1 条文本（仅 query）。
- 新增 1 个 chunk 后：恰好 1 条文本被向量化。
- 切换 `ai.embedding_model` 后：全部 chunk 重新向量化一次，之后回到稳态。
- 检索结果**不变**：同一语料下改造前后 top-15 的 `chunk_id` 集合完全一致。
- 向量化失败不污染已有向量（保持 `store_embedding` 覆盖语义）。

### benchmark
新增 `src-tauri/tests/perf_retrieval.rs`（`#[ignore]`，release 跑）：
```
for n in [1_000, 10_000, 50_000]:
    seed n chunks + 预热一次全量 embedding
    t0 = Instant::now(); semantic_search_with_usage(...); t1
    记录：耗时 / embedding 输入 token / provider.embed 调用次数与入参长度
断言：第 2、3 次调用的 embedding token == 1 次 query 的长度
```

---

## PERF-02 Vector Top-K 检索  P0  ✅ 已完成

**一句话**：有界堆只留 top-K，且分两阶段取正文。

### 修改文件
- `src-tauri/src/infrastructure/embedding_repository.rs`（重写 `nearest_chunks`）

### SQL
阶段一（只取 id + 向量，**不 JOIN 正文**）：
```sql
SELECT chunk_id, embedding FROM chunk_embeddings WHERE model = ?1;
```
阶段二（按 top-K id 取正文，**分批以规避 SQLite 变量上限 999**）：
```sql
SELECT id, content FROM chunks WHERE id IN (?1, ?2, ..., ?n);  -- n <= 900
```

### 算法
- `BinaryHeap<Reverse<(OrderedF32, String)>>` 维护**容量 K 的有界小顶堆**，
  复杂度 `O(N·D + N·log K)`、内存 `O(K)`，取代现在的 `O(N·D + N log N)` / `O(N)`。
- 按 `model` 过滤（让 `idx_chunk_embeddings_model` 真正生效）。
- 相似度仍用现有 `cosine_similarity`，**不改变打分语义**（诚实优先）。
- K 由调用方传入（Ask 传 15）。

### 验收指标
- **等价性单测**：固定随机种子生成 N=5000、K=15 向量，新实现与「全量排序」的
  top-K 输出完全一致。
- `EXPLAIN QUERY PLAN` 显示使用了 `idx_chunk_embeddings_model`（不再全表扫）。
- 峰值内存不随 N 增长（K 固定时堆大小恒定）。
- 维度不匹配 / 零向量仍返回 0.0，不 panic（沿用现有诚实语义）。

### benchmark
`src-tauri/tests/perf_vector.rs`（`#[ignore]`）：
```
for n in [10_000, 50_000, 100_000]:
    t0; nearest_chunks(query, 15); t1
    记录耗时 + 采样进程 RSS 峰值
对比改造前（全量 sort + JOIN content）与改造后
```

---

## PERF-03 SQLite 连接策略  P1  🟡 部分完成（WAL 已做，连接池待做）

**一句话**：WAL 只设一次；连接池替代「每 IPC 开一个连接」。

### 修改文件
- `src-tauri/src/infrastructure/db.rs`（拆分 `open` / `init_db`，WAL 移入一次性初始化）
- `src-tauri/src/lib.rs`（`AppState` 增加 pool 字段）
- `src-tauri/src/commands/*.rs`（`state.open()?` → 取连接守卫）

### SQL / PRAGMA
```rust
// 一次性（仅当 DB 首次创建 / 升级时执行）
PRAGMA journal_mode = WAL;    // 数据库级持久设置，不该每连接重发

// 每连接（连接级参数）
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
```

### 进度

**✅ 已完成：WAL 一次化**（commit `ad590cc`）。`open()` 只留连接级 PRAGMA，
`enable_wal()` 由 `initialize()` 调一次并校验返回值。测试证明「后续连接不再发该
PRAGMA 也依然是 WAL 模式」。

**⬜ 未完成：连接池。** 原因与风险，需单独一步：
- `AppState::open()` 目前返回 `Connection`；改为池守卫（Deref 到 Connection）后，
  **约 30 处** `service(conn, …)` 都要改成 `service(&conn, …)`，diff 面远大于前面几项。
- 真正的风险在异步命令：Tauri 的 async 命令里**绝不能**把连接守卫跨 `.await` 持有
  （`start_extraction` / `start_research` 已经是 async 且会 await 闸门），
  一旦误持有会跨线程持有连接，轻则 busy 报错、重则句柄失效。
- 建议实现顺序：① 先加池并保持 `open()` 签名不变（内部复用）；② 再逐步迁移调用点；
  ③ 最后收紧 API 禁止跨 await 持有。每步都可独立回滚。

### 算法（连接池部分，待做）
- 引入小型连接池（建议 `r2d2` + `r2d2_sqlite`，或自建 `Mutex<Vec<Connection>>`），
  容量 2~4：WAL 下「多读 + 单写」互不阻塞。
- `AppState::conn()` 返回连接守卫；服务层签名尽量仍为 `&Connection`
  （守卫 `Deref<Target=Connection>`，调用点由 `service(conn, …)` 改为 `service(&conn, …)`）。
- **边界**：异步命令（`start_extraction`）**不得**把守卫跨 `.await` 持有；
  后台管线继续用自己的 `db::open`（保持现有 `spawn_blocking` 结构不变）。

### 验收指标
- 启动日志中 `journal_mode` 的设置动作只出现 1 次（用测试计数器断言）。
- 冷启动加载首页的 IPC 耗时 p50 下降（对照 benchmark）。
- 并发压测：6 读 + 1 写持续 10s，`SQLITE_BUSY` 计数为 0。
- `apply_migrations` 与 `SCHEMA_VERSION` 断言不变（既有测试全绿）。

### benchmark
`src-tauri/tests/perf_sqlite.rs`（`#[ignore]`）：
```
并发 6 线程只读 list_claims + 1 线程循环写 candidate，各 10s
记录：总耗时 / p99 延迟 / SQLITE_BUSY 次数
```

---

## PERF-04 索引与查询  P1  ✅ 已完成

**一句话**：按真实查询补索引；候选列表改游标分页。

### SQL（迁移 `0013_perf_indexes.sql`，`SCHEMA_VERSION → 13`）—— 已执行

```sql
-- ① 服务 trace_service::get_claim_trace：
--    WHERE accepted_claim_id = ?1 ORDER BY created_at DESC LIMIT 1
--    该列此前**完全没有索引**——「Claim 溯源」用得越久越慢的根因。
CREATE INDEX IF NOT EXISTS idx_candidates_accepted_claim
  ON candidates(accepted_claim_id);

-- ② 服务候选游标分页：
--    WHERE run_id = ?1 AND (created_at, id) > (?, ?) ORDER BY created_at, id LIMIT ?
--    同时是 idx_candidates_run 的严格前缀超集，故 DROP 旧索引。
CREATE INDEX IF NOT EXISTS idx_candidates_run_cursor
  ON candidates(run_id, created_at, id);

-- ③ 服务 claim_repository::CLAIM_SELECT 挑「主证据」的窗口函数：
--    ROW_NUMBER() OVER (PARTITION BY claim_id ORDER BY evidence_level, created_at)
--    让 SQLite 沿索引序走，免掉全表 evidence 的临时排序。
--    同时是 idx_evidence_claim 的前缀超集，故 DROP 旧索引。
CREATE INDEX IF NOT EXISTS idx_evidence_claim_level_created
  ON evidence(claim_id, evidence_level, created_at);

DROP INDEX IF EXISTS idx_candidates_run;
DROP INDEX IF EXISTS idx_evidence_claim;
```

### 推迟的索引（审计建议但**当前无查询支撑**）

审计列了 5 条，实际核查后只有 3 条值得现在建。剩下两条是**投机性索引**——
加了只白付写入成本与库体积，等真有对应查询时再补：

| 推迟的索引 | 核查结果 | 何时该补 |
| --- | --- | --- |
| `candidates(document_id, status, created_at DESC)` | `candidates` 现有查询只有 `WHERE run_id` / `WHERE id` / `WHERE accepted_claim_id`，**没有**按 document+status 的 | 出现「按文档筛候选」的 Review/Inbox 需求时 |
| `runs(parent_run_id, started_at)` | 全代码库**零**处 `WHERE parent_run_id = ?`（已 grep 确认） | 真正实现 Run 树（Agent → Skill → Tool）下钻时 |
| `reviews(target_type, target_id)` | `review_repository` 只按 `status` / `id` 查询 | 出现「按目标取 Review」的需求时 |

> 纪律：**索引必须有对应查询才建**。每个索引都是持续的写入放大，
> 「以防将来会用」不是理由。

### 修改文件
- 新增 `src-tauri/migrations/0013_perf_indexes.sql` + `db.rs`（`MIGRATION_0013`、`SCHEMA_VERSION`）
- `src-tauri/src/infrastructure/claim_repository.rs`（复核 `CLAIM_SELECT` 的窗口函数 + 相关 `COUNT(*)` 子查询能否去掉）
- `src-tauri/src/infrastructure/candidate_repository.rs`（新增游标分页）
- `src-tauri/src/commands/candidate.rs` + 前端 `ExtractionPanel`（分页拉取）

### 算法
- 候选分页用**游标**而非 `OFFSET`（候选会持续增长）：
  ```sql
  SELECT ... FROM candidates
  WHERE run_id = ?1
    AND (created_at, id) > (?2, ?3)   -- 复合游标
  ORDER BY created_at, id
  LIMIT ?4;                          -- 默认 50
  ```

### 验收指标
- `get_claim_trace` 的 `EXPLAIN QUERY PLAN` 不出现 `SCAN candidates`。
- 1 万条候选下分页首屏 < 50ms。
- 分页遍历 10 万条候选总耗时为线性（无 O(n²) 偏移退化）。
- 索引不得改变查询语义（既有测试全绿）。

### benchmark
`src-tauri/tests/perf_query.rs`（`#[ignore]`）：
```
造 10k claims / 10k candidates / 10k evidence
对 get_claim_trace / list_claims / 分页遍历 分别记录耗时
附改造前后的 EXPLAIN QUERY PLAN 文本快照
```

---

## PERF-05 抽取内存与并发  P1

**一句话**：`result_json` 不再存全部候选；AI 后台任务加统一并发上限。

### 修改文件
- `src-tauri/src/application/extraction_service.rs`（不再累积 `all`）
- `src-tauri/src/application/dto.rs`（`ExtractionReport.extracted` → 摘要）
- 新增 `src-tauri/src/ai/scheduler.rs`（并发闸门）
- `src-tauri/src/ai/provider.rs`（复用 `reqwest::blocking::Client`）

### 结构变更
`extraction_runs.result_json` 从「全部候选」改为**摘要**：
```json
{
  "candidate_count": 132,
  "directly": 108, "partially": 17, "unsupported": 7,
  "duplicates": 21, "changes_estimate": 45
}
```
候选明细**只从 `candidates` 表取**（已有 `list_by_run`，将加分页）。
**兼容注意**：`extract_claims`（同步预览）当前依赖 `extracted` 列表——保持其返回完整
列表；只让**后台 Run 的 result_json** 变摘要。前端 `HomePage` / `ExtractionPanel`
已改用 `list_candidates`，需回归验证。

### 并发闸门
在**命令 / 编排层**加统一调度（服务层保持纯业务，不感知并发）：
```
extraction : 1~2     embedding : 1     ask : 2~4     agent/skill : 2
```
用 `tokio::sync::Semaphore`，`acquire` 后再 `spawn_blocking`。
产品是单用户 Local-first，目标是**整机响应稳定**，不是并发最大化。

### 复用 HTTP client
`provider.rs` 三处 `reqwest::blocking::Client::new()` → 进程内共享一个 `Client`
（保留连接池 / keep-alive 收益）。

### 验收指标
- 500-chunk 文档抽取全程峰值 RSS 下降（对照 benchmark）。
- `result_json` 体积 < 1KB（原来随候选数线性增长）。
- 同时发起「抽取 + 2 次 Ask + 1 次 Research」：无 `SQLITE_BUSY`、无 API 429。
- 抽取产出的候选条数与改造前**完全一致**（正确性不回退）。

### benchmark
`src-tauri/tests/perf_extraction.rs`（`#[ignore]`）：
```
构造 500-chunk 文档 + 假 provider（不发真实请求）
采样线程每 50ms 读 /proc/self/status 的 VmRSS，输出峰值
同时统计 result_json 字节数
```

---

## PERF-06 IPC 载荷与前端查询  P1

**一句话**：减少 IPC 次数与载荷；列表一律分页、剔除重字段。

### 修改文件
- 新增聚合命令 `src-tauri/src/commands/overview.rs`
- `src-tauri/src/lib.rs`（注册命令）
- 前端 `src/lib/api.ts` + 各页面

### 做法
1. **首页冷加载聚合**：现为 `app_info` + `list_registries` + `list_documents` +
   `list_review_items` + `list_extraction_runs` + `get_settings`（≥6 次 IPC，
   每次还要开 SQLite 连接）。合并为一个 `get_home_overview`，返回
   `{ appInfo, pendingReview, recentRuns, registries }`。
2. **列表剔除重字段**（部分已落地）：
   - `list_extraction_runs` 不再下发 `result_json` —— 已随性能修复提交
   - `list_candidates` 分页 —— 见 PERF-04
3. **Trace 按需下钻**：`get_claim_trace` 当前一次返回（数据量小）；
   为未来 `get_trace_segment(kind)` 预留接口（PR-02 的逐层下钻已满足体验）。

### 验收指标
- 首页冷加载 IPC 次数 ≤ 2。
- 首页冷加载总载荷（字节）下降 ≥ 50%。
- 页面切换无「多次骨架屏闪烁」（合并请求后一次到位）。

### benchmark
手工统计：每页冷加载的 `invoke` 次数与响应字节数，写入本单验收表。

---

## 执行顺序与依赖

```
PERF-01 ─┐
         ├─ 改同一文件（embedding_repository / retrieval_service），必须串行
PERF-02 ─┘

PERF-03（连接池）── 可与 PERF-04/05 并行，但会放大后者的 diff，建议排在后
PERF-04（索引/分页）── 独立，可并行
PERF-05（抽取内存/并发）── 独立，可并行
PERF-06（IPC 聚合）── 依赖 PERF-04 的分页形态
```

**推荐节奏**

1. 先做 **PERF-01 + PERF-02**（P0，收益最大、风险最低、不改 schema）。
2. 再做 **PERF-04**（索引 + 分页；含迁移 `0013`）。
3. 然后 **PERF-05**（抽取内存 + 并发闸门 + 共享 HTTP client）。
4. 接着 **PERF-03**（连接池；diff 面最大，安排在功能稳定后）。
5. 最后 **PERF-06**（IPC 聚合，依赖分页形态定型）。

## 全局验收（PERF 完成后）

| 指标 | 改造前 | 目标 |
| --- | --- | --- |
| 10k chunk 稳态 Ask 的 embedding token | O(N) | O(1)（仅 query） |
| 100k chunk 向量检索耗时 | 基线 | 不劣于 10k 的 10 倍（近似线性） |
| 向量检索峰值内存 | O(N) | O(K) |
| 首页冷加载 IPC 次数 | ≥6 | ≤2 |
| 抽取 500-chunk 峰值 RSS | 基线 | 明显下降 |
| 并发 6 读 + 1 写 | 可能 BUSY | 0 次 BUSY |

## 每项任务完成后的固定动作

1. `cargo test --lib`（既有 334 个测试必须全绿——**正确性优先于性能**）。
2. 该项的 `#[ignore]` benchmark 在 `--release` 下跑一次，把数字回填到本单。
3. 单独提交一个 commit，message 引用本单编号（如 `perf(PERF-01): …`）。
4. 诚实边界变化时，同步更新本单与 `docs/约束.md`。
