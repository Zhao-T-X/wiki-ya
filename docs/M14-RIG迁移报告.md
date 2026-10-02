# M14：Rig 迁移完成报告

> 日期：2026-10-02 ｜ main @ PR5（8f6f3c8）｜ 测试 327/327

## 结果

按 8-PR 计划完成 Rig 正式引入（PR6-7 经核实已由既有实现覆盖，见下）。

| PR | 内容 | 提交 |
|---|---|---|
| 1 | rig-core 0.42 依赖 + `ai/rig_adapter.rs`（AgentRequest/AgentResult 契约 + 单轮补全） | `1902a09` |
| 2 | `run_react_blocking` 多轮循环：同一 JSON 动作协议 / 12 白名单工具 / 8 轮上限 / Policy 闸门 + Golden 对比测试 | `e3b64da` |
| 3 | Research 调用方切换；Legacy runtime 保留为 Golden 基线 | `1d01dda` |
| 4 | 自定义 Skill 切 RigAdapter；rig 路径接入流式专用端点记忆 | `b7cf94c` |
| 5 | Extraction 主路径切 RigAdapter（流式兜底覆盖最终用户场景） | `8f6f3c8` |

## 边界（行动计划的落地）

- **Rig 管"怎么跑"**：provider / 消息 / 补全 / 流式，全部收敛在
  `ai/rig_adapter.rs`——业务层零直接依赖。
- **wiki-ya 管"能对知识做什么"**：Skill 注册表与版本、Policy 三级闸门、
  candidates 持久化、Run/Trace 全在 Adapter 之外，未交给框架。

## PoC 已验证并沿用的三条事实

1. rig 0.42 默认 Responses API，兼容端点必须 `.completions_api()`；
2. 需 rustc ≥ 1.88（let-chains），上游不声明 MSRV；
3. rig 全异步 ↔ 同步调用方：临时 current-thread runtime 桥接。

## PR6-7 核实结论

- **Policy Hook**：M5 闸门已在 Rig 循环内强制（`tools::execute(..., &role.policy())`，
  权限不足 → Err 回填模型），枚举级"AI 永不 MUTATE"测试固化——Rig Hook
  的 Skip/Terminate 在当前单工具闸门模式下无增量价值，暂不引入。
- **事件接线**：循环内已发完整 RunEvent（Started/ToolCalled/ToolCompleted/
  Completed/Failed），经 `run-events` 单频道到前端。

## 待验证（需真实 Key）

```bash
cd src-tauri && cargo test --lib golden -- --ignored --nocapture
```

对比 Legacy vs Rig 的工具序列与答案；通过后 Legacy 可在任意时点退役。
