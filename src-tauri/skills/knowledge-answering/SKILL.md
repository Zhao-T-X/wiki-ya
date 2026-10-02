---
name: knowledge-answering
version: 1
description: 基于知识库回答问题，回答带来源引用
permissions: [read]
tools: [search_knowledge, get_claim, get_entity, get_evidence]
input: |
  { "question": "问题", "role": "auto | knowledge | research | curator | personal" }
output: |
  { "answer": "回答", "sources": [编号来源], "enabled": AI 是否启用 }
---

## Instructions

基于检索到的知识回答问题：

1. 只使用上下文中提供的知识条目，每条论断标注来源编号 `[n]`。
2. 知识库中没有的内容，如实回答「知识库中未找到」，绝不编造。
3. 引用的条目数与来源列表严格对齐，方便用户逐条下钻。

## 诚实边界

- 回答是 READ 操作的产物：不改任何知识，也不产生候选。
- 检索结果超出上下文预算被截断时，说明这一点。
