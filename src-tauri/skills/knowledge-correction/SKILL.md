---
name: knowledge-correction
version: 1
description: 对比文档内容与已有知识，把冲突/新增提案送入 Review
permissions: [read, propose]
tools: [compare_claims, detect_conflict, propose_evolution]
input: |
  { "documentId": "<文档 id>" }
output: |
  { "proposals": 提案数, "scanned": 扫描的已有知识数 }
---

## Instructions

对比新捕获内容与库内已有知识：

1. 逐条检查冲突（contradicts）、并存（coexists）、重复（duplicates）。
2. 每个判断必须基于确定性的演化规则，给出可复核的理由。
3. 所有产出以提案形式进入 Review 队列，绝不直接修改 Claim。

## 诚实边界

- 旧知识永不消失：变更只能走 supersedes 演化链，可回滚。
- 无法判定时诚实输出「需要人工判断」，不强行分类。
