---
name: knowledge-extraction
version: 1
description: 从一篇文档中抽取结构化的候选知识（只预览，不落库）
permissions: [read, propose]
tools: []
input: |
  { "documentId": "<文档 id>" }
output: |
  { "candidates": 已接受的候选数, "total": 候选总数, "enabled": AI 是否启用 }
---

## Instructions

从给定文本中抽取严谨的结构化 Claim：

1. 只使用受控词表中的 predicate，绝不发明新谓语。
2. 每条 Claim 必须能在原文切片中找到依据，`sentence` 字段回引原文。
3. 离散字段只用受控取值：普通事实陈述一律 `asserted`，不要输出系词。
4. 置信度低于 0.5 的断言直接丢弃，宁缺毋滥。

## 诚实边界

- 候选**只预览、不落库**；确认与否由用户在 Review 决定。
- 文本无可抽取内容时输出空集合，不编造。
