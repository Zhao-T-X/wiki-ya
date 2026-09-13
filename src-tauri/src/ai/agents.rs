//! Agent 角色与系统提示（Phase 6 骨架，TDD §52）。
//!
//! 这里只定义角色枚举与每个角色的系统提示词；真正的工具调用循环在
//! `runtime` / `tools` 中（TDD §49-§51）。Ask 当前直接走 `KnowledgeAgent`
//! 的提示词 + 白名单工具的结果，不做完整 ReAct 循环（后续增强）。

/// Agent 角色（TDD §52）。用户默认 `auto`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    Auto,
    Personal,
    Knowledge,
    Research,
    Curator,
    Review,
    Extraction,
}

impl AgentRole {
    pub fn from_str(value: &str) -> Self {
        match value {
            "personal" => AgentRole::Personal,
            "knowledge" => AgentRole::Knowledge,
            "research" => AgentRole::Research,
            "curator" => AgentRole::Curator,
            "review" => AgentRole::Review,
            "extraction" => AgentRole::Extraction,
            _ => AgentRole::Auto,
        }
    }

    /// 该角色对应的系统提示词。
    pub fn system_prompt(&self) -> &'static str {
        match self {
            AgentRole::Knowledge | AgentRole::Auto => KNOWLEDGE_PROMPT,
            AgentRole::Personal => PERSONAL_PROMPT,
            AgentRole::Research => RESEARCH_PROMPT,
            AgentRole::Curator => CURATOR_PROMPT,
            AgentRole::Review => REVIEW_PROMPT,
            AgentRole::Extraction => EXTRACTION_PROMPT,
        }
    }
}

const KNOWLEDGE_PROMPT: &str = "你是 wiki-ya 的 Knowledge Agent——一个严格基于用户本地知识库作答的助手。\
只使用提供的上下文段落作答；对每条事实用 [n] 标注其来源段落编号。\
如果上下文中没有答案，明确说「知识库中未找到相关信息」，绝不编造或猜测。\
清晰区分「来自知识库的事实」与「模型的一般性推断」。回答用中文。";

const PERSONAL_PROMPT: &str = "你是 wiki-ya 的 Personal Agent，帮助用户管理个人知识库的目标与计划。\
严格基于知识库内容，不编造。";

const RESEARCH_PROMPT: &str = "你是 wiki-ya 的 Research Agent，负责针对一个问题做跨来源的研究与综述。\
基于提供的上下文，给出带引用的综述；缺失信息明确说明。";

const CURATOR_PROMPT: &str = "你是 wiki-ya 的 Curator Agent，负责知识库的整洁与去重。\
基于上下文提出合并、归一化建议，不擅自修改数据。";

const REVIEW_PROMPT: &str = "你是 wiki-ya 的 Review Agent，负责审核知识变更（重复、矛盾、演化）。\
基于上下文给出判定与理由。";

const EXTRACTION_PROMPT: &str = "你是 wiki-ya 的 Extraction Agent，从文本中抽取结构化 Claim。\
只使用受控谓语词表。";
