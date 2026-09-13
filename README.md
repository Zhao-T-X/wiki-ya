# wiki-ya

一个以 Ontology 为核心、以 Evidence 为依据、以 Evolution 为历史、以 Context Efficiency 为 AI 基础设施的
**Local-first Personal Knowledge System**（Tauri 2 + React 18 + TypeScript + Rust + SQLite）。

设计规格见 `docs/`：

| 文档 | 内容 |
|---|---|
| `docs/wiki-ya需求说明书.md` | 产品需求（PRD v2.0） |
| `docs/技术设计文档.md` | 技术设计与实现规格（TDD v2.0） |
| `docs/领域枚举与不变量定义.md` | 受控词表、24 条不变量、8 项待裁决决策 |
| `docs/IPC契约.md` | 前端 ↔ Rust 的唯一接口约定 |
| `docs/项目分析报告.md` | 现状分析与缺口闭合情况 |

---

## 当前进度

**已完成：Phase 0–5（Domain / Evolution / Search / Desktop UX / Context Efficiency）+ Phase 6（Ask / Agent 运行时 / 工具调用 / 流式输出 / Research）+ Phase 4 余项（Timeline）+ Phase 8（存量迁移）。**

跑通的能力：

- **Capture**：写入原文 → 确定性切分切片 → 建立 FTS5（trigram）索引，**全程不调用 AI**
- **Knowledge**：实体（含别名消解）、Claim、Evidence 的浏览；**手动录入 Claim**（无 API Key 的降级路径）
- **Search**：文档 / Claim / 实体三路词法检索 + RRF 融合，并回答「为什么这条匹配了」
- **语义检索（Ask 路径）**：chunk 级向量（OpenAI 兼容 embeddings）+ 余弦近邻，与词法结果 RRF 融合；
  不可用时诚实降级为纯词法，绝不伪造相似度。
  ⚠️ **Search 页仍为纯词法**：勾选「语义检索」时会降级并在界面展示后端返回的 `notice` 说明。
- **Evolution**：`compare_claim` 确定性关系判定（duplicate / coexists / contradicts），自动确认不改变知识的关系
- **Review**：四个必答问题（What changed / Why / Evidence / Impact）+ 接受 / 保持两者 / 拒绝；
  `superseded` 状态迁移的**唯一入口**，支持精确回滚
- **Ask**：基于知识库提问，回答带 `[n]` 编号引用与 Sources 下钻，
  附 Context Stats（预算 / 压缩比 / 截断）透明面板
- **Context Efficiency**：token 估算（CJK/ASCII 启发式）、planner 加载策略、超限压缩、预算贪心装桶
- **AI 配置化**：API Key / 接口基址 / Chat 模型 / 向量模型均可在 Settings 页配置并持久化
  （优先级：设置页 > 环境变量 > 默认值），保存即生效
- **Knowledge Health**：全部为真实计数，不做估算
- **Settings**：受控词表浏览器（前端不硬编码任何枚举值）

**AI 抽取（Phase 5a）**：

- 文档详情页可「抽取 Claim」：模型输出经**受控词表校验**后只做预览，
  逐条/批量「接受」才调用既有 `create_claim` + `analyze_document` 落库并触发演化分析。
- **未配置 Key 时诚实降级**：`app_info.ai_enabled = false`，抽取 / Ask 接口返回 `enabled:false`
  与说明，UI 如实展示「AI 未启用」，**绝不伪造**任何输出。
- ⚠️ **模型选择**：抽取 / 检索请使用**非推理型**模型（如 `gpt-4o-mini`、`deepseek-chat`）。
  推理型模型（如 `deepseek-reasoner`、部分 `*-flash`）会把正文放进 `reasoning_content`
  而让 `content` 为空，导致抽取拿不到结果。代码已加兜底（重试 / 流式 / 思维链抢救），
  但**不保证稳定**，推荐直接使用非推理模型。

**尚未实现（有意留空，不伪造）**：

- MCP Server（Knowledge / Research / Review 工具）→ Phase 7
- Context 遥测的 History / Trace 回放 UI（`context_runs` / `context_cache` 已落库，仅缺前端视图）

> UI 中所有涉及 AI 的位置都明确标注「未启用」。这是刻意的：一个诚实说"我不知道"的系统，
> 比一个编造答案的系统有用得多。

---

## 运行

前置：Node ≥ 22、pnpm、Rust ≥ 1.86、Xcode Command Line Tools（macOS）。

```bash
pnpm install

# 浏览器里预览界面（数据库不可用，会显示提示条）
pnpm dev

# 启动桌面应用（推荐）
pnpm tauri:dev

# 打包
pnpm tauri:build

# 只检查类型
pnpm typecheck
```

启用 AI（可选，不配置则系统诚实降级为「未启用」）：

推荐：启动应用后在 **Settings → AI 运行时** 里配置 API Key / 接口基址 / Chat 模型 / 向量模型
（持久化保存，立即生效，无需重启）。

也可以用环境变量预设（优先级低于设置页）：

```bash
export WIKIYA_API_KEY=sk-...
# 可选：export WIKIYA_BASE_URL=https://api.openai.com/v1
# 可选：export WIKIYA_MODEL=gpt-4o-mini
# 可选：export WIKIYA_EMBEDDING_MODEL=text-embedding-3-small
pnpm tauri:dev
```

Rust 侧单独验证（包含全部单元测试）：

```bash
cd src-tauri
cargo test
```

### 通过 `pnpm tauri:dev` 试一遍完整闭环

1. **Inbox** 粘贴一段文字 → 捕获（会显示切片数）
2. 进入 **Knowledge**，在右侧手动新建两条互相冲突的 Claim
   （同一主语 + 同一谓语 + 不同宾语，例如 `OpenAI` / `is` / `Sam` 与 `OpenAI` / `is` / `Alice`）
3. 回到 **Inbox** 点该文档的「分析演化」→ 生成待确认的 `contradicts`
4. 进入 **Review** → 把它改成 `supersedes` 并接受 → 旧 Claim 变为历史
5. 在 **Knowledge** 里仍能看到旧 Claim（标记为历史），内容一字未改
6. **Search** 里搜关键词 → 结果会说明命中了哪些字段

---

## 架构

依赖方向（TDD §86，不可违反）：

```text
UI  → Commands → Application → Domain
                                  ↑
         Infrastructure ──────────┘   （只实现 Domain 需要的持久化）
AI Runtime → Application → Domain
```

```text
src/                      React 前端（8 个一级模块 + 命令面板）
src-tauri/
├── registries/           受控词表（从参考实现原样迁移，编译期内嵌）
├── migrations/           纯 DDL，幂等，由 db.rs 执行
└── src/
    ├── domain/            Ontology / Knowledge / Evidence / Evolution / Graph / Review / Search
    ├── application/       用例编排 + 事务边界 + IPC DTO
    ├── infrastructure/    SQLite / FTS5 / Repository
    ├── commands/          Tauri IPC（只做 Deserialize → Service → Serialize）
    ├── events/            应用事件（UI 流式、审计、调试）
    └── ai/                Phase 5/6 预留
```

**禁止**：`Domain → SQLite / Tauri / React`、`Agent → SQL`、`直接 UPDATE claim.status`。

---

## 开发说明

- `src-tauri/.cargo/config.toml` 把 crates-io 指向 USTC 的 **sparse** 索引。
  本机全局 `~/.cargo/config` 用的是已废弃的 `git://` 协议，在当前网络下无法解析依赖。
  修好全局配置后可以直接删除该文件。
- `Cargo.toml` 里 `rust-version = "1.86"` 与 `resolver = "3"` 是必需的：
  Tauri 的部分传递依赖（`time` / `icu` / `serde_with` 等）会要求 rustc 1.88+，
  MSRV 感知解析会把它们锁到兼容版本。**不要随意删除这两行**。
- 所有受控枚举的取值必须与 `registries/*.json` 逐字一致：
  启动时 `registry::self_check()` 会断言这一点，不一致直接启动失败。
- 数据库校验用 `PRAGMA integrity_check` / `foreign_key_check`；
  FTS5 使用 `tokenize='trigram'`（`unicode61` 会让中文检索失效，不要改）。
