-- ============================================================================
-- wiki-ya 初始 Schema（0001）
--
-- 执行方式：Rust 侧用 rusqlite::Connection::execute_batch 一次性执行。
-- 因此本文件是 **纯 DDL，不含任何 PRAGMA**：外键开关 / WAL / synchronous /
-- busy_timeout 由 db.rs 在打开连接时统一设置。
--
-- 幂等性：应用每次启动都会重复执行本文件，所以每个对象都必须带
-- IF NOT EXISTS（表 / 索引 / 唯一索引 / 触发器 / 虚拟表均是）。
--
-- 时间的写法：统一 TEXT + datetime('now')，存 UTC。用 datetime('now')
-- 而非 CURRENT_TIMESTAMP 是为了与 Rust 侧写入的时间戳格式（'YYYY-MM-DD
-- HH:MM:SS'，UTC）保持字节级可比，避免索引与范围查询出现两种格式。
--
-- 枚举字面量的唯一依据：docs/领域枚举与不变量定义.md。
-- 所有 CHECK 的取值必须与该文档逐字一致，改动前先改文档。
-- ============================================================================

-- ---------------------------------------------------------------------------
-- 元信息
-- ---------------------------------------------------------------------------
-- 记录已应用的迁移版本，支撑 schemaVersion（IPC 契约 AppInfo）。
-- 应用迁移前先查 version，避免重复执行破坏性语句。
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---------------------------------------------------------------------------
-- 原始文档（Rule 1: Raw is immutable；INV-01）
-- ---------------------------------------------------------------------------
-- Document 是不可变原文：结构化知识只能引用它，永不覆盖它（INV-01）。
-- 因此这里没有"被知识回写"的列，title/content 只由用户显式编辑或导入写入。
CREATE TABLE IF NOT EXISTS documents (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  source_type TEXT NOT NULL DEFAULT 'note',
  source_uri TEXT,
  -- content_hash = SHA-256(content)。UNIQUE 就是 INV-02 的实现手段：
  -- 相同原文只能存在一份 Document，重复导入在数据库层直接冲突（IPC → CONFLICT）。
  content_hash TEXT NOT NULL UNIQUE,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
-- 按最近更新倒序列文档列表（Inbox 主视图）。
CREATE INDEX IF NOT EXISTS idx_documents_updated ON documents(updated_at DESC);

-- chunks 是原文的切片视图，offset 指回 documents.content，是"原文的坐标"
-- 而非原文的替代品（INV-01）。文档删除时切片级联清除，不会留下悬空引用。
CREATE TABLE IF NOT EXISTS chunks (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  chunk_index INTEGER NOT NULL,
  start_offset INTEGER NOT NULL,
  end_offset INTEGER NOT NULL,
  content TEXT NOT NULL,
  char_count INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  -- INV-23：同文档内切片下标唯一，保证切分结果可复现、可重排。
  UNIQUE(document_id, chunk_index)
);
-- 覆盖"取某文档全部切片"这一主查询路径。
CREATE INDEX IF NOT EXISTS idx_chunks_document ON chunks(document_id);

-- ---------------------------------------------------------------------------
-- 实体
-- ---------------------------------------------------------------------------
-- primary_type 为注册表主类型（取 types_json[0]，见 §1.1），
-- 这里是存储快照；合法性由领域层对照 Registry 校验（INV-13），
-- 不落成 CHECK，以免把 14 值注册表复制出第二份真相。
CREATE TABLE IF NOT EXISTS entities (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  primary_type TEXT NOT NULL,
  types_json TEXT NOT NULL DEFAULT '[]',
  description TEXT,
  properties_json TEXT NOT NULL DEFAULT '{}',
  -- 状态受 CHECK 约束（INV-12）；与 §3 的五值集合逐字一致。
  status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('draft','candidate','verified','rejected','archived')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
-- INV-03：实体规范名大小写不敏感唯一。用 lower(name) 建唯一索引，
-- 使 "OpenAI" 与 "openai" 在数据库层就不可能并存两份，而不是靠调用方自觉。
CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_name_ci ON entities(lower(name));
-- 支撑按类型过滤实体列表（list_entities(entityType)）。
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(primary_type);

-- 别名独立成表，**不设 aliases_json 列**：别名只有一个真相来源（INV-04）。
-- 同实体下别名唯一；不同实体可以共享同一别名（多义实体各自持有），
-- 这正是 PRIMARY KEY(entity_id, alias_normalized) 而非全局唯一的原因。
CREATE TABLE IF NOT EXISTS entity_aliases (
  entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  alias TEXT NOT NULL,
  alias_normalized TEXT NOT NULL,
  PRIMARY KEY(entity_id, alias_normalized)
);
-- INV-04/INV-13 之外：支撑 Entity Resolution 的 Alias 查表（alias → entity_id）。
CREATE INDEX IF NOT EXISTS idx_entity_alias_normalized ON entity_aliases(alias_normalized);

-- ---------------------------------------------------------------------------
-- Claim / Evidence
-- ---------------------------------------------------------------------------
-- INV-05：Claim 不修改、不删除，只演进——所以这里没有"被覆盖"的列，
-- 演化只发生在 status 与 claim_relations。
-- INV-06：subject_id / predicate / object_id / content 在演化中恒不改写。
-- 来源信息（文档 / 切片 / 引文）**不放这里**，那是 Evidence 的职责
-- （PRD §17 / Rule 3），从而"一条 claim 多份证据"不需要扩列。
CREATE TABLE IF NOT EXISTS claims (
  id TEXT PRIMARY KEY,
  subject_id TEXT NOT NULL REFERENCES entities(id),
  predicate TEXT NOT NULL,
  object_id TEXT REFERENCES entities(id),
  object_text TEXT,
  content TEXT,
  context_json TEXT NOT NULL DEFAULT '{}',
  -- claim_type 受 CHECK 约束，与 §2.2 的 8 值集合逐字一致。
  claim_type TEXT NOT NULL DEFAULT 'factual' CHECK(claim_type IN ('factual','definitional','causal','comparative','evaluative','predictive','normative','hypothetical')),
  polarity TEXT NOT NULL DEFAULT 'positive' CHECK(polarity IN ('positive','negative')),
  -- modality 六值，与 §2.4 一致。决策 D3：conditional 不进 modality 枚举，
  -- 条件由下方独立 condition 列承载，避免同一语义有两处表达。
  modality TEXT NOT NULL DEFAULT 'asserted' CHECK(modality IN ('asserted','possible','probable','capable','necessary','recommended')),
  condition TEXT,
  -- INV-18：confidence 恒在 [0,1] 或为 NULL（未知不猜，用 NULL 表达）。
  confidence REAL CHECK(confidence IS NULL OR confidence BETWEEN 0 AND 1),
  -- 六值含 superseded。§2.1：superseded 是历史，rejected 是错误，两者必须区分，
  -- 这是"优先当前知识、仍能回答历史问题"的前提。INV-08 保证 superseded 只由
  -- claim_relations 的 supersedes+accepted 产生。
  status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('draft','candidate','verified','rejected','archived','superseded')),
  valid_from TEXT,
  valid_until TEXT,
  recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
  created_by TEXT NOT NULL DEFAULT 'system',
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_claims_subject ON claims(subject_id);
CREATE INDEX IF NOT EXISTS idx_claims_object ON claims(object_id);
CREATE INDEX IF NOT EXISTS idx_claims_predicate ON claims(predicate);
CREATE INDEX IF NOT EXISTS idx_claims_status ON claims(status);
-- 主力索引：Current Knowledge 派生与候选检索都按 (subject, predicate) 收窄，
-- 再按 status 过滤 superseded（§4.7）。三列联合让该路径免于回表。
CREATE INDEX IF NOT EXISTS idx_claims_sp ON claims(subject_id, predicate, status);

-- Evidence 是 Claim 与原文之间的唯一桥（Rule 3）。chunk_id 用 SET NULL：
-- 切片可从原文重建（TDD §85），重建不应连带删除证据本身。
CREATE TABLE IF NOT EXISTS evidence (
  id TEXT PRIMARY KEY,
  claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  chunk_id TEXT REFERENCES chunks(id) ON DELETE SET NULL,
  start_offset INTEGER,
  end_offset INTEGER,
  quote TEXT,
  -- §6.5：证据层级 1..5（quote / quote+context / paragraph / chunk / multi-chunk）。
  evidence_level INTEGER NOT NULL DEFAULT 1 CHECK(evidence_level BETWEEN 1 AND 5),
  source_type TEXT NOT NULL DEFAULT 'note',
  confidence REAL CHECK(confidence IS NULL OR confidence BETWEEN 0 AND 1),
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_evidence_claim ON evidence(claim_id);
CREATE INDEX IF NOT EXISTS idx_evidence_document ON evidence(document_id);
CREATE INDEX IF NOT EXISTS idx_evidence_chunk ON evidence(chunk_id);

-- ---------------------------------------------------------------------------
-- Claim 演化（唯一入口见 INV-08 / INV-09）
-- ---------------------------------------------------------------------------
-- source = 新 claim（发起方），target = 旧 claim（被影响方）（§4.5）。
-- relationship 六值 = D1 裁决的并集：PRD 的 supplements + 参考实现的 unclear。
-- 此处**保留** unclear，因为它是候选关系的合法落库值（人工可读）；
-- 自动判定路径不写它，属应用层约束（INV-11）。
-- INV-07：同一对 claim 只留一行 —— UNIQUE(source,target,relationship) 阻止重复上报。
CREATE TABLE IF NOT EXISTS claim_relations (
  id TEXT PRIMARY KEY,
  source_claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  target_claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  relationship TEXT NOT NULL CHECK(relationship IN ('duplicate','coexists','supplements','supersedes','contradicts','unclear')),
  status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('candidate','accepted','rejected')),
  confidence REAL CHECK(confidence IS NULL OR confidence BETWEEN 0 AND 1),
  reason TEXT,
  suggested_action TEXT,
  -- INV-09 的存储基础：target claim 被取代前的原状态。取消 supersedes 时按它
  -- 精确回滚，而不是猜一个默认值——"精确恢复"必须有落库的锚点才可能做到。
  target_previous_status TEXT,
  created_by TEXT NOT NULL DEFAULT 'system',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  reviewed_at TEXT,
  UNIQUE(source_claim_id, target_claim_id, relationship),
  -- 自指关系无意义，数据库层直接拒绝（与方向约定无关的完整性护栏）。
  CHECK(source_claim_id <> target_claim_id)
);
CREATE INDEX IF NOT EXISTS idx_claim_rel_source ON claim_relations(source_claim_id);
CREATE INDEX IF NOT EXISTS idx_claim_rel_target ON claim_relations(target_claim_id);
-- 支撑 Review 队列按状态取件并按时间倒序展示（§4.3 优先级由应用层排序）。
CREATE INDEX IF NOT EXISTS idx_claim_rel_status ON claim_relations(status, created_at DESC);

-- ---------------------------------------------------------------------------
-- 实体关系（Entity → Entity，与 Claim 不混淆；§1.4 / INV-15 / INV-16）
-- ---------------------------------------------------------------------------
-- 只有归一化 outcome = DIRECT_RELATION 的谓词才允许写这张表（INV-15），
-- always_claim_only_predicates 永不写入（INV-16）——均为应用层硬规则，
-- 表结构只负责"关系两端是实体、且带证据"这一事实。
-- 决策 D6：status **不含 superseded**。实体→实体关系不存在"被取代"语义，
-- 取代语义只属于 Claim 的历史演进；保留这一点可避免两套生命周期混淆。
CREATE TABLE IF NOT EXISTS relations (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES entities(id),
  predicate TEXT NOT NULL,
  target_id TEXT NOT NULL REFERENCES entities(id),
  context_json TEXT NOT NULL DEFAULT '{}',
  confidence REAL CHECK(confidence IS NULL OR confidence BETWEEN 0 AND 1),
  status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('draft','candidate','verified','rejected','archived')),
  -- 关系证据原地内联（不像 Claim 那样外置），因为关系本身不是被演进对象：
  -- 文档/切片删除时仅置空证据指针，关系事实不随之消失。
  evidence_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
  evidence_chunk_id TEXT REFERENCES chunks(id) ON DELETE SET NULL,
  evidence_quote TEXT,
  created_by TEXT NOT NULL DEFAULT 'system',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  -- INV-07 同源：同一 (source, predicate, target) 只允许存在一条关系，
  -- 避免抽取/审核时重复生成同一条边。
  UNIQUE(source_id, predicate, target_id)
);
-- 覆盖"查某条 (source,predicate,target) 关系是否存在"的写入前校验路径（INV-07 同源思想）。
CREATE INDEX IF NOT EXISTS idx_rel_key ON relations(source_id, predicate, target_id);
CREATE INDEX IF NOT EXISTS idx_rel_source ON relations(source_id);
CREATE INDEX IF NOT EXISTS idx_rel_target ON relations(target_id);

-- ---------------------------------------------------------------------------
-- 其他知识对象
-- ---------------------------------------------------------------------------
-- event_type 18 值与 §3 逐字一致（默认 other）。
CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  event_type TEXT NOT NULL CHECK(event_type IN ('creation','development','release','publication','deployment','acquisition','merger','migration','training','evaluation','experiment','update','decision','announcement','meeting','failure','incident','other')),
  description TEXT NOT NULL,
  participants_json TEXT NOT NULL DEFAULT '[]',
  time_json TEXT NOT NULL DEFAULT '{}',
  location TEXT,
  -- 六值状态，默认 unknown（不确定比假装确定更诚实）。
  status TEXT NOT NULL DEFAULT 'unknown' CHECK(status IN ('planned','ongoing','completed','cancelled','failed','unknown')),
  confidence REAL CHECK(confidence IS NULL OR confidence BETWEEN 0 AND 1),
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 决策 D4：Idea 状态以参考实现为准（5 值，已落库 + 有测试）。
CREATE TABLE IF NOT EXISTS ideas (
  id TEXT PRIMARY KEY,
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'candidate' CHECK(status IN ('candidate','accepted','implemented','rejected','archived')),
  confidence REAL CHECK(confidence IS NULL OR confidence BETWEEN 0 AND 1),
  source_document_id TEXT REFERENCES documents(id),
  source_chunk_id TEXT REFERENCES chunks(id),
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 决策 D4：Question 状态以参考实现为准（status 6 值 / question_type 5 值）。
CREATE TABLE IF NOT EXISTS questions (
  id TEXT PRIMARY KEY,
  content TEXT NOT NULL,
  question_type TEXT NOT NULL DEFAULT 'knowledge' CHECK(question_type IN ('knowledge','research','design','implementation','evaluation')),
  status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','answered','partially_answered','resolved','rejected','archived')),
  source_document_id TEXT REFERENCES documents(id),
  source_chunk_id TEXT REFERENCES chunks(id),
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 问题被删除不应连带删除已开展的研究（产物独立于来源），故 SET NULL。
CREATE TABLE IF NOT EXISTS research_tasks (
  id TEXT PRIMARY KEY,
  question_id TEXT REFERENCES questions(id) ON DELETE SET NULL,
  question_text TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','running','completed','failed')),
  findings TEXT,
  run_id TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_research_tasks_created ON research_tasks(created_at DESC);

-- ---------------------------------------------------------------------------
-- 审核
-- ---------------------------------------------------------------------------
-- target_id 是多态外键（指向 claim/entity/relation/... 之一），SQLite 无法
-- 声明多态外键，所以只用 CHECK 锁定 target_type 的合法取值（INV-12）。
CREATE TABLE IF NOT EXISTS reviews (
  id TEXT PRIMARY KEY,
  target_type TEXT NOT NULL CHECK(target_type IN ('claim','claim_relation','entity','relation','evidence','research_finding','agent_proposal')),
  target_id TEXT NOT NULL,
  proposal_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','accepted','rejected','superseded')),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  reviewed_at TEXT
);
-- 支撑 Review 队列按状态取件、最新优先。
CREATE INDEX IF NOT EXISTS idx_reviews_status ON reviews(status, created_at DESC);

-- ---------------------------------------------------------------------------
-- 派生数据（TDD §85：可重建，不可作为真相；INV-22）
-- ---------------------------------------------------------------------------
-- 以下所有表都可以在不丢失 Documents/Claims/Evidence/Relations/History 的
-- 前提下被清空重建。它们只是缓存与遥测，不是知识本身。
--
-- 嵌入表（chunk_embeddings）迁移到 0002_embeddings.sql：语义检索的向量，
-- 属派生数据，chunk 删除即失效，重算即可恢复。

-- Context 运行遥测（TDD §79）。
CREATE TABLE IF NOT EXISTS context_runs (
  id TEXT PRIMARY KEY,
  run_id TEXT,
  agent_name TEXT NOT NULL,
  budget_tokens INTEGER NOT NULL DEFAULT 0,
  actual_tokens INTEGER NOT NULL DEFAULT 0,
  trimmed_tokens INTEGER NOT NULL DEFAULT 0,
  efficiency REAL,
  over_budget INTEGER NOT NULL DEFAULT 0,
  optimizations_json TEXT NOT NULL DEFAULT '[]',
  withheld_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_context_runs_agent ON context_runs(agent_name, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_context_runs_run ON context_runs(run_id);

-- 每个 section 是编译结果的一个片段；policy 四值与 §6.3 逐字一致。
CREATE TABLE IF NOT EXISTS context_sections (
  id TEXT PRIMARY KEY,
  context_run_id TEXT NOT NULL REFERENCES context_runs(id) ON DELETE CASCADE,
  section_index INTEGER NOT NULL,
  name TEXT NOT NULL,
  policy TEXT NOT NULL CHECK(policy IN ('LOAD','SUMMARIZE','RETRIEVE_LATER','NEVER_LOAD')),
  source TEXT,
  reason TEXT,
  tokens INTEGER NOT NULL DEFAULT 0,
  chars INTEGER NOT NULL DEFAULT 0,
  trimmed INTEGER NOT NULL DEFAULT 0,
  UNIQUE(context_run_id, section_index)
);
CREATE INDEX IF NOT EXISTS idx_context_sections_run ON context_sections(context_run_id, section_index);

CREATE TABLE IF NOT EXISTS task_packets (
  id TEXT PRIMARY KEY,
  goal TEXT,
  context_summary TEXT,
  agent TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  estimated_tokens INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_task_packets_created ON task_packets(created_at DESC);

-- 压缩后的会话状态：每个 (conversation, agent) 一行，只保存滚动摘要 + 近期窗口，
-- 不保存完整 transcript（避免把派生数据当真相）。
CREATE TABLE IF NOT EXISTS conversation_summaries (
  conversation_id TEXT NOT NULL,
  agent TEXT NOT NULL DEFAULT 'PersonalAgent',
  summary TEXT NOT NULL DEFAULT '',
  recent_json TEXT NOT NULL DEFAULT '[]',
  messages_seen INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY(conversation_id, agent)
);
CREATE INDEX IF NOT EXISTS idx_conversation_summaries_updated ON conversation_summaries(updated_at DESC);

-- INV-20：cache_key 是 prompt/skill/reference/registry/schema 版本的组合哈希，
-- 任一版本变化即生成新 key，旧条目"按构造"失效，无需手工清理（INV-21 同理）。
CREATE TABLE IF NOT EXISTS context_cache (
  cache_key TEXT PRIMARY KEY,
  agent TEXT NOT NULL DEFAULT '',
  prompt_json TEXT NOT NULL,
  hits INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  last_hit_at TEXT
);

-- ---------------------------------------------------------------------------
-- 运行可观测（TDD §79）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS agent_runs (
  id TEXT PRIMARY KEY,
  task_type TEXT NOT NULL CHECK(task_type IN ('extract','ask','agent')),
  document_id TEXT REFERENCES documents(id) ON DELETE CASCADE,
  agent_role TEXT,
  model TEXT,
  status TEXT NOT NULL CHECK(status IN ('started','success','failed')),
  step_count INTEGER NOT NULL DEFAULT 0,
  summary_json TEXT NOT NULL DEFAULT '{}',
  error_message TEXT,
  duration_ms INTEGER,
  prompt_tokens INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_runs_created ON agent_runs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runs_type ON agent_runs(task_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runs_document ON agent_runs(document_id);

-- run 内步骤序号唯一；run 删除即级联清除，不留孤儿步骤。
CREATE TABLE IF NOT EXISTS agent_events (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
  step_index INTEGER NOT NULL,
  name TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('started','success','failed')),
  input_summary TEXT,
  output_text TEXT,
  error_message TEXT,
  duration_ms INTEGER,
  prompt_tokens INTEGER,
  completion_tokens INTEGER,
  usage_source TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(run_id, step_index)
);
CREATE INDEX IF NOT EXISTS idx_agent_events_run ON agent_events(run_id, step_index);

-- ---------------------------------------------------------------------------
-- 全文检索
-- ---------------------------------------------------------------------------
-- 必须是 external-content（content='documents'）的 FTS5，靠下面三个触发器
-- 保持与 documents 同步（INV-24）。
--
-- 为什么 tokenize='trigram'（不要改成 unicode61）：
--   unicode61 会把一整段连续中文当成**单个 token**，于是标题《苹果的SEO是乔布斯》
--   整段无法被拆开检索，中文检索形同虚设。
--   trigram 建立的是「三字符滑动窗口」索引，中文与英文一样退化为子串匹配：
--   搜「苹果的」能命中《苹果的SEO是乔布斯》，搜 "async" 能命中 "async fn in trait"。
--   这是参考实现踩坑后的修正结论，wiki-ya 必须沿用。
--
-- 实测边界（SQLite 3.51，务必知悉，勿当成 bug）：trigram 的查询词**至少 3 个字符**。
--   2 字符的中文查询（如「苹果」）在 MATCH 下命中为空——这是 trigram 的固有语义，
--   没有索引配置可以绕过，也不能为此换回 unicode61。
--   检索层对 <3 字符的查询词应走 `documents.title/content LIKE '%词%'` 兜底（结果正确，
--   但会退化为全表扫描）。3 字符及以上一律优先用 MATCH，走 trigram 索引。
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
  title,
  content,
  content='documents',
  content_rowid='rowid',
  tokenize='trigram'
);

-- INV-24：FTS 与 documents 最终一致。三个触发器覆盖 INSERT/DELETE/UPDATE。
-- external-content 表的删除/更新必须向 FTS 发送 'delete' 命令并回传旧值，
-- 否则残留的旧 token 会污染索引（搜得到已不存在的文档）。
CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents BEGIN
  INSERT INTO documents_fts(rowid,title,content) VALUES (new.rowid,new.title,new.content);
END;

CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents BEGIN
  INSERT INTO documents_fts(documents_fts,rowid,title,content)
  VALUES ('delete',old.rowid,old.title,old.content);
END;

CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents BEGIN
  INSERT INTO documents_fts(documents_fts,rowid,title,content)
  VALUES ('delete',old.rowid,old.title,old.content);
  INSERT INTO documents_fts(rowid,title,content)
  VALUES (new.rowid,new.title,new.content);
END;
