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

**已完成：Phase 0–5（Domain / Evolution / Search / Desktop UX / Context Efficiency）+ Phase 6（Ask / Agent 运行时 / 工具调用 / 流式输出 / Research）+ Phase 4 余项（Timeline）+ Phase 8（存量迁移）+ MCP Server。**

> 产品形态已完成一轮 **使用体验重构**（详见 `docs/使用体验重构.md`）：
> 导航从 10 个平级入口收敛为 **Capture + Home / Knowledge / Review / Search**
> （低频的 Research / Graph / Timeline / Migration 收进「更多」），Search 与 Ask 合并为同一入口。

跑通的能力：

- **Capture（Home）**：写入原文 → 确定性切分切片 → 建立 FTS5（trigram）索引；捕获本身**不调用 AI**。
  配置了 AI 时，捕获后**自动抽取知识候选**并给出结果反馈（逐条确认后才落库）。
- **Knowledge**：实体（含别名消解）、Claim、Evidence 的浏览；**手动录入 Claim**（无 API Key 的降级路径）；
  详情页按 What / Why current / Evidence / History / Related 组织，并解释「为什么它是当前知识」。
- **Search / Ask（统一入口）**：文档 / Claim / 实体三路词法检索 + RRF 融合，**知识优先**展示，
  并回答「为什么这条匹配了」；可一键让 AI 基于同一批检索作答（Answer + `[n]` 引用 + Sources 下钻）。
- **语义检索**：chunk 级向量（OpenAI 兼容 embeddings）+ 余弦近邻，与词法结果 RRF 融合；
  不可用时诚实降级为纯词法，绝不伪造相似度，并在界面展示后端返回的 `notice` 说明。
- **Evolution**：`compare_claim` 确定性关系判定（duplicate / coexists / contradicts），自动确认不改变知识的关系
- **Review（系统待办）**：四个必答问题（What changed / Why / Evidence / Impact）+ 接受 / 保持两者 / 拒绝；
  `superseded` 状态迁移的**唯一入口**，支持精确回滚。待办数量常驻侧边栏。
- **Research**：研究任务编排与报告卡片；结果只进 Review，不直接落库。
- **Timeline**：按时间轴回看知识事件。
- **Migration**：存量数据迁移（探测 → 执行 → 报告）。
- **Graph**：实体关系图（实体详情内展示）。
- **MCP Server**：独立二进制（`cargo run --bin mcp`），暴露 5 个**只读**工具
  （`search_knowledge` / `get_entity` / `get_claim` / `get_evidence` / `knowledge_health`），
  供外部 MCP 客户端接入。失败一律以 `isError: true` 诚实返回。
- **Context Efficiency**：token 估算（CJK/ASCII 启发式）、planner 加载策略、超限压缩、预算贪心装桶；
  Context Stats（预算 / 压缩比 / 截断）默认收起，需要审计时再展开。
- **AI 配置化**：API Key / 接口基址 / Chat 模型 / 向量模型均可在 Settings 页配置并持久化
  （优先级：设置页 > 环境变量 > 默认值），保存即生效；默认只暴露「模型 + API Key」，其余在「高级」。
- **Knowledge Health**：全部为真实计数，不做估算
- **Settings**：受控词表（Ontology）只读浏览，默认折叠在「高级」下（前端不硬编码任何枚举值）

**AI 抽取（Phase 5a）**：

- 捕获后**自动抽取**（AI 已启用时），也可在文档详情页手动「抽取 Claim」：模型输出经**受控词表校验**
  后只做预览，逐条/批量「接受」才调用既有 `create_claim` + `analyze_document` 落库并触发演化分析。
- **未配置 Key 时诚实降级**：`app_info.ai_enabled = false`，抽取 / Ask 接口返回 `enabled:false`
  与说明，UI 如实展示「AI 未启用」，**绝不伪造**任何输出。
- ⚠️ **模型选择**：抽取 / 检索请使用**非推理型**模型（如 `gpt-4o-mini`、`deepseek-chat`）。
  推理型模型（如 `deepseek-reasoner`、部分 `*-flash`）会把正文放进 `reasoning_content`
  而让 `content` 为空，导致抽取拿不到结果。代码已加兜底（重试 / 流式 / 思维链抢救），
  但**不保证稳定**，推荐直接使用非推理模型。

**尚未实现（有意留空，不伪造）**：

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

推荐：启动应用后在 **Settings → AI 运行时** 里配置 **模型 + API Key**
（接口基址 / 向量模型 / 上下文预算在「高级」里，使用官方接口时无需改动）。
持久化保存，立即生效，无需重启。

> **API Key 的安全存储**：Key **不以明文写入 SQLite**，而是用 **AES-256-GCM** 加密后
> 存进 `settings` 表的 `ai.api_key.enc`（每次加密使用独立随机 nonce）。
> 主密钥为 32 字节随机值，存放于 `<app-data-dir>/secret.key`，文件权限 `0600`。
> 接口基址 / 模型 / 预算等**非敏感**配置仍以明文存储（它们不是机密）。
>
> ⚠️ **安全边界（如实说明）**：主密钥与数据库位于同一用户目录下，因此本方案能防
> 「数据库文件被单独复制 / 误传 / 误提交」导致的 Key 泄露，但**不能**防能够读取你
> home 目录的本地攻击者（他能连主密钥一起拿走）。需要更强的静态加密时，应改用
> 「用户口令派生密钥」或系统级密钥库。
>
> 升级路径：检测到旧库里的明文 `ai.api_key` 时，启动时自动加密迁移
> （写入后**读回校验**通过才删除明文）；迁移失败则保留原值并记录警告，不影响使用。
> 日志与错误信息中的所有 Key 均已脱敏。
>
> 注：早期版本使用的 macOS Keychain 方案**已移除**。若你的 Key 只存在于 Keychain 中，
> 升级后需要**重新输入一次**（旧 Keychain 条目不再被读取，可在「钥匙串访问」里手动删除）。

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

1. **Home** 粘贴一段文字 → 捕获（会显示切片数；已配置 AI 时会自动抽取候选）
2. 进入 **Knowledge**，手动新建两条互相冲突的 Claim
   （同一主语 + 同一谓语 + 不同宾语，例如 `OpenAI` / `is` / `Sam` 与 `OpenAI` / `is` / `Alice`）
3. 打开该文档，运行「检查知识变化 / 分析演化」→ 生成待确认的 `contradicts`
4. 进入 **Review**（侧边栏会显示待办数量）→ 把它改成 `supersedes` 并接受 → 旧 Claim 变为历史
5. 在 **Knowledge** 里仍能看到旧 Claim（标记为历史），内容一字未改
6. **Search** 里搜关键词（或直接提问）→ 结果**知识优先**，并说明命中了哪些字段

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
src/                      React 前端（Capture + Home / Knowledge / Review / Search + 更多 + 命令面板）
src-tauri/
├── registries/           受控词表（从参考实现原样迁移，编译期内嵌）
├── migrations/           纯 DDL，幂等，由 db.rs 执行
└── src/
    ├── domain/            Ontology / Knowledge / Evidence / Evolution / Graph / Review / Search
    ├── application/       用例编排 + 事务边界 + IPC DTO
    ├── infrastructure/    SQLite / FTS5 / Repository
    ├── commands/          Tauri IPC（只做 Deserialize → Service → Serialize）
    ├── events/            应用事件（UI 流式、审计、调试）
    ├── ai/                Provider / Runtime / Agents / Tools（已实现）
    ├── logging.rs         零依赖极简日志（`WIKIYA_LOG` 控制级别）
    └── bin/mcp.rs         独立 MCP Server 二进制（只读工具）
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
