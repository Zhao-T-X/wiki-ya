# wiki-ya

## Product Requirements & Functional Specification v2.0

> Project: wiki-ya
> Product Type: Local-first AI Personal Knowledge System
> Platform: Desktop
> Status: Implementation Specification

---

# 1. 产品定位

## 1.1 一句话

**wiki-ya 是一个能够理解、关联、验证和持续演进个人知识的 Local-first AI Knowledge System。**

不是传统 Wiki。

不是单纯笔记软件。

不是 Chatbot。

也不是单纯 Knowledge Graph。

它的核心是：

```text
Capture
  ↓
Understand
  ↓
Connect
  ↓
Evolve
  ↓
Verify
  ↓
Retrieve
```

---

# 2. 核心产品理念

## 2.1 Capture first. Organize later.

用户不应该在输入知识时决定：

* 这是 Entity？
* 这是 Claim？
* 这是 Idea？
* 这是 Question？
* 应该放哪个分类？
* 应该建立什么 Relation？

用户只需要输入。

系统负责理解。

---

## 2.2 Never silently overwrite knowledge.

新知识永远不能直接覆盖旧知识。

必须：

```text
New Claim
    ↓
Compare Existing Claims
    ↓
Evolution Relation
```

关系包括：

```text
duplicate
coexists
supplements
supersedes
contradicts
```

历史永久保留。

---

## 2.3 Evidence is part of knowledge.

知识不是：

```text
Claim
```

而是：

```text
Claim
 +
Evidence
 +
Source
 +
Time
```

任何重要知识都应该能够回答：

> 为什么你认为这是真的？

---

## 2.4 AI suggests, user decides.

AI 可以：

* 抽取
* 搜索
* 判断
* 推荐
* 总结
* 提议

但是：

```text
AI
 ↓
Proposal
 ↓
Domain Validation
 ↓
User Review
 ↓
Commit
```

AI 不直接拥有知识库写权限。

---

## 2.5 Deterministic logic first.

程序负责：

* Schema
* Ontology
* Predicate Registry
* Normalization
* Entity Resolution
* Evidence Validation
* Relation Direction
* Evolution State
* Transaction
* Cache Version

LLM 负责：

* Semantic Extraction
* Semantic Comparison
* Natural Language Reasoning
* Research Synthesis

---

## 2.6 Complexity belongs in the system, not the UI.

用户不需要理解：

```text
Ontology Registry
Context Compiler
Task Packet
Claim Relation
Evidence Escalation
RRF
Cache Fingerprint
```

这些都是系统内部能力。

用户应该看到：

```text
这条知识可能更新了已有知识。
```

而不是：

```text
请选择 supersedes relation。
```

---

# 3. 用户核心闭环

```text
                    ┌──────────────┐
                    │    Capture   │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │ Understanding│
                    └──────┬───────┘
                           ↓
                ┌─────────────────────┐
                │ Existing Knowledge  │
                └──────────┬──────────┘
                           ↓
                    ┌──────────────┐
                    │   Evolution  │
                    └──────┬───────┘
                           ↓
        ┌──────────────────┼──────────────────┐
        ↓                  ↓                  ↓
     New/Duplicate     Supplement         Conflict
        │                  │                  │
        └──────────────────┼──────────────────┘
                           ↓
                         Review
                           ↓
                         Commit
                           ↓
              Knowledge / Graph / Search
                           ↓
                      Ask / Research
                           ↓
                       New Capture
```

---

# 4. 一级产品模块

wiki-ya 一级导航：

```text
Inbox
Knowledge
Search
Review
Ask
Research
Graph
Settings
```

不把 Agent、Ontology、Context Runtime 暴露成一级导航。

---

# 5. Inbox

Inbox 是所有知识进入系统的入口。

支持：

* 文本输入
* Markdown
* TXT
* HTML
* 文件导入
* Web Clip
* 粘贴内容
* AI 对话
* MCP
* 外部 Agent

---

## 5.1 Inbox 状态

```text
captured
processing
understood
review_required
committed
failed
archived
```

---

## 5.2 输入体验

用户输入：

```text
Rust 现在已经支持 async fn in trait。
```

系统后台：

```text
Parse
 ↓
Extract Claim
 ↓
Resolve Rust
 ↓
Normalize Predicate
 ↓
Search Existing Claims
 ↓
Compare
```

UI 最终显示：

```text
发现相关知识

新知识：
Rust supports async fn in trait

可能影响已有知识：

Rust does not support async fn in trait

建议：
Supersedes

[接受] [保留两者] [编辑]
```

---

# 6. Document / Raw

原始文档永远保留。

支持：

```text
Markdown
TXT
HTML
PDF（后续）
Web Clip
Plain Text
```

Document 是 Source of Truth。

结构化知识永远不能覆盖 Document。

---

# 7. Chunk

Chunk 是内部计算单位。

包含：

```text
document_id
start_offset
end_offset
content
hash
embedding
```

用户默认不操作 Chunk。

只有在查看 Evidence 时进入 Chunk。

---

# 8. Ontology

Ontology 是 wiki-ya Domain Core。

核心对象：

```text
Entity
Concept
Claim
Predicate
Relation
Event
Idea
Question
```

---

# 9. Entity

Entity 表示稳定对象。

例如：

```text
Rust
Tauri
React
SQLite
AgentScope
OpenAI
```

Entity 支持：

* canonical name
* aliases
* type
* metadata
* status

Entity Resolution：

```text
Exact
 ↓
Alias
 ↓
Normalized
 ↓
Fuzzy
 ↓
Semantic
```

LLM 只能作为最后一级建议。

---

# 10. Entity Type

使用受控 Entity Type Registry（封闭集合，版本化）。

不要允许 LLM 自由创造无限 Entity Type。

Registry 版本化。

例如：

```text
Person
Organization
Product
Software
Technology
Concept
Place
Event
...
```

具体类型以迁移阶段从旧 registry 原样迁移。

---

# 11. Claim

Claim 是知识的核心单位。

```text
subject
predicate
object
polarity
modality
claim_type
confidence
status
valid_from
valid_until
recorded_at
```

例如：

```text
Rust
supports
async fn in trait
```

---

# 12. Predicate Registry

Predicate 必须受控。

例如：

```text
supports
depends_on
created_by
part_of
uses
replaces
related_to
```

Predicate 必须定义：

```text
id
name
domain
range
inverse
symmetry
transitivity
conflict_rules
normalization_rules
```

---

# 13. Relation

Relation 表达 Entity → Entity。

例如：

```text
wiki-ya
uses
SQLite
```

Relation 与 Claim 不混淆。

---

# 14. Event

Event 表达发生过的事情。

例如：

```text
Rust 1.85 released
Project migrated
Tauri 2 released
```

Event 包含：

```text
subject
event_type
time
evidence
```

---

# 15. Idea

Idea 表达尚未成为事实或知识的思考。

例如：

```text
可以把 wiki-ya 做成 MCP Server。
```

Idea 不强制转 Claim。

状态：

```text
active
developing
converted
archived
```

---

# 16. Question

Question 表达未解决问题。

例如：

```text
AgentScope Rust 是否足够成熟？
```

Question 可以进入 Research。

状态：

```text
open
investigating
resolved
partially_resolved
archived
```

---

# 17. Evidence

Evidence 是 Claim 与 Source 之间的桥。

```text
Claim
 ↓
Evidence
 ↓
Chunk
 ↓
Document
```

Evidence 保存：

```text
document_id
chunk_id
start_offset
end_offset
quote
source
confidence
evidence_level
```

---

# 18. Evidence Escalation

保留原项目 L1-L5。

```text
L1 Summary
 ↓
L2 Claim
 ↓
L3 Evidence
 ↓
L4 Chunk
 ↓
L5 Original Source
```

核心原则：

> 不需要的信息不进入 Context。

用户点击：

```text
为什么？
```

系统才逐层展开。

---

# 19. Claim Evolution

Claim 不覆盖。

关系：

```text
duplicate
coexists
supplements
supersedes
contradicts
```

例如：

```text
Claim A
Rust does not support async fn in trait

       ↓ supersedes

Claim B
Rust supports async fn in trait
```

---

# 20. Temporal Knowledge

Claim 增加：

```text
valid_from
valid_until
recorded_at
```

这样：

```text
历史上正确
```

与：

```text
当前正确
```

可以同时存在。

---

# 21. Current Knowledge

Current Knowledge 不是单独存储的一份复制数据。

它由：

```text
Claim Status
+
Claim Evolution
+
Temporal Validity
```

派生。

因此：

```text
History ≠ Current State
```

---

# 22. Conflict

Conflict 必须经过：

```text
Candidate Retrieval
 ↓
Entity Resolution
 ↓
Predicate Matching
 ↓
Polarity
 ↓
Temporal Context
 ↓
Ontology Rule
 ↓
Semantic Comparison
```

Vector Similarity 不直接等于 Conflict。

---

# 23. Retrieval

统一 Search：

```text
FTS5
+
Vector
+
Ontology
+
Graph
```

排序：

```text
RRF
+
Relevance
+
Confidence
+
Recency
+
Evidence
```

---

# 24. Search UX

统一搜索：

```text
Search everything...
```

结果：

```text
Knowledge
Claims
Entities
Documents
Events
Ideas
Questions
Research
```

---

# 25. Graph

Graph 保留。

但不作为默认首页。

Graph 用于：

```text
Entity Detail
→ Related Knowledge
→ Explore Graph
```

支持：

* Entity Neighborhood
* Global Graph
* Depth
* Predicate Filter
* Status Filter

---

# 26. Ask

Ask 是知识库问答。

回答必须：

```text
Answer
+
Citations
+
Evidence
+
Context Stats
```

用户可以点击：

```text
Source
 ↓
Chunk
 ↓
Claim
```

---

# 27. Research

Research 是多步知识构建。

流程：

```text
Question
 ↓
Research Task
 ↓
Search
 ↓
Knowledge Agent
 ↓
Task Packet
 ↓
Research Agent
 ↓
Evidence
 ↓
Findings
 ↓
Candidate Knowledge
 ↓
Review
```

研究结果不能自动污染 Knowledge。

---

# 28. Agent

内部继续保留：

```text
PersonalAgent
KnowledgeAgent
ResearchAgent
CuratorAgent
ReviewAgent
ExtractionAgent
```

但是 UI 不要求用户选择 Agent。

默认：

```text
role = auto
```

由 Router 判断。

---

# 29. Agent Runtime

AgentScope 负责：

```text
Reasoning
Planning
Tool Calling
Agent Lifecycle
Streaming
State
```

AgentScope Rust 当前提供 Agent、Tool、State、Middleware 等基础能力，并提供 AG-UI adapter，因此可直接作为 wiki-ya AI Runtime 的基础。

---

# 30. Context Efficiency Engine

这是 wiki-ya 的核心基础设施。

```text
Context Efficiency Engine

├── Token Budget
├── Context Planner
├── Context Compiler
├── Context Pack
├── Retrieval
├── Deduplication
├── Compression
├── History Compression
├── Tool Compression
├── Evidence Escalation
├── Context Cache
├── Tool Cache
├── Prefix Cache
└── Context Trace
```

---

# 31. Context Loading Policy

每个 Context Item 有：

```text
LOAD
SUMMARIZE
RETRIEVE_LATER
NEVER_LOAD
```

例如：

```text
Ontology Rules
→ LOAD / subset

Relevant Claim
→ LOAD

Original Document
→ RETRIEVE_LATER

SQLite Schema
→ NEVER_LOAD
```

---

# 32. Context Pack

Agent 不直接接收 Knowledge Database。

接收：

```text
ContextPack
```

包含：

```text
goal
entities
claims
evidence
summaries
references
budget
estimated_tokens
omitted_items
```

---

# 33. Token Budget

每次 Agent Run：

```text
System
History
Knowledge
Evidence
Tools
Output
```

都有独立预算。

例如：

```text
System       2K
History      2K
Knowledge    4K
Evidence     4K
Tools        1K
Output       2K
----------------
Total       15K
```

---

# 34. Tool Result Compression

Tool 不返回大对象。

例如 Search：

```text
id
title
type
score
status
summary
```

需要详情：

```text
get_knowledge(id)
```

Tool 输出：

```text
Decision Card
```

而不是完整数据库对象。

---

# 35. Batch Tool

例如：

```text
get_entities([1,2,3,4])
```

替代：

```text
get_entity(1)
get_entity(2)
get_entity(3)
get_entity(4)
```

减少：

* Tool Calls
* Schema Tokens
* Round Trips

---

# 36. History Compression

Conversation：

```text
Recent 4 messages
→ Full

Older messages
→ Deterministic Summary

Very old
→ Semantic Memory / Reference
```

Summary：

```text
goal
decisions
facts
open_questions
references
```

---

# 37. Task Packet

Agent 之间：

> 传状态，不传完整上下文。

TaskPacket：

```text
task_id
goal
context_summary
known_facts
open_questions
constraints
required_actions

entity_ids
claim_ids
evidence_ids
document_ids

required_output
```

---

# 38. Context Cache

Cache Key：

```text
Prompt Version
+
Skill Version
+
Reference Version
+
Ontology Registry Version
+
Schema Version
+
History Version
+
Task Packet Version
```

任意依赖改变：

```text
New Fingerprint
→ New Cache Key
```

无需手工 invalidation。

---

# 39. Prompt / Skill

保留：

```text
Core Contract
+
Skill
+
Reference
+
Custom Prompt
```

Reference 默认 Lazy Load。

Skill metadata：

```text
estimated_tokens
load_when
```

---

# 40. Context Inspector

高级用户可以看到：

```text
Budget
Actual
Efficiency
Trimmed
Cached
```

以及：

```text
Section
Strategy
Why Loaded
Token Cost
Source
```

并显示：

```text
Intentionally Not Loaded
```

---

# 41. Knowledge Health

提供：

```text
Potential Duplicates
Unresolved Conflicts
Claims Without Evidence
Unresolved Entities
Stale Knowledge
Rejected Candidates
```

例如：

```text
Knowledge Health

12 possible duplicates
5 unresolved conflicts
18 claims without evidence
7 unresolved entities
3 stale claims
```

---

# 42. Review Center

统一 Review：

```text
New Knowledge
Duplicate
Supplement
Supersede
Conflict
Entity Merge
Research Finding
Agent Proposal
```

每个 Review 必须解释：

```text
What changed?
Why?
Evidence?
Impact?
```

---

# 43. Backup / Export

支持：

```text
SQLite
JSON
Markdown
```

原则：

> 用户拥有自己的知识。

---

# 44. Local-first

无 API Key 时仍可：

```text
Capture
Document
Chunk
FTS Search
Graph
Review
Knowledge CRUD
Export
```

AI 功能可关闭。

---

# 45. Settings

只暴露必要设置：

```text
Model
Embedding
AI Provider
Token Budget
AgentScope
Indexing
Storage
Backup
```

高级设置：

```text
Context Inspector
Prompt Profiles
Skills
Registry
Runtime Trace
```

---

# 46. MVP

## P0

```text
Tauri
Rust Core
SQLite
Document
Chunk
Entity
Claim
Evidence
Ontology Registry
Entity Resolution
Claim Evolution
Review
FTS5
Local Vector
Hybrid Search
```

## P1

```text
Inbox
Knowledge Detail
Timeline
Conflict UX
Ask
Graph
Research
Knowledge Health
Context Efficiency
```

## P2

```text
AgentScope
MCP
Advanced Context Inspector
Prompt / Skill Manager
Advanced Research
```

---

# 47. Non-goals

当前不做：

```text
多人协作
云同步
插件市场
企业权限
复杂 SaaS Backend
大型知识图谱平台
Agent Marketplace
Multi-user Workspace
```

---

# 48. 成功标准

用户应该可以：

1. 快速记录知识
2. 不需要自己分类
3. 自动发现相关知识
4. 自动发现可能冲突
5. 一眼看到知识演进
6. 一键查看证据
7. 快速搜索
8. 向自己的知识库提问
9. 进行 Research
10. 不担心 AI 覆盖历史
11. 不担心 Context Token 浪费

---

# 49. 最终产品模型

```text
              wiki-ya

                Inbox
                  ↓
             Understanding
                  ↓
              Ontology
                  ↓
        ┌─────────┼─────────┐
        ↓         ↓         ↓
      Entity    Claim      Event
                  ↓
              Evidence
                  ↓
           Claim Evolution
                  ↓
        ┌─────────┼─────────┐
        ↓         ↓         ↓
     Search      Graph     Review
        ↓                    ↓
       Ask                 Commit
        ↓                    ↓
     Research ←──────── Knowledge
```

---

# 50. 产品最终定位

> **wiki-ya 是一个以 Ontology 为核心、以 Evidence 为依据、以 Evolution 为历史、以 Context Efficiency 为 AI 基础设施的 Local-first Personal Knowledge System。**
