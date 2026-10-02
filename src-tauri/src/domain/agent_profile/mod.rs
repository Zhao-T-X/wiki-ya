//! Agent Profile —— Agent 的声明式配置（M4）。
//!
//! Agent 定义的是「我拥有哪些能力、被允许做什么」：
//! - `skills` 引用已注册的 Skill（`skills` 表），执行时按声明顺序编排；
//! - `policy` 是该 Agent 的权限上限（read / propose），Skill 的
//!   permissions 必须是其子集才会被实际执行（M5 闸门雏形）；
//! - `model` 为空表示跟随全局 AI 设置。
//!
//! 执行底座说明（M4 决策）：Profile 编排是**确定性的顺序执行**，
//! 不需要 LLM 驱动的 ReAct 循环——自研 `ai::runtime` 继续承担自由
//! 问答 / 研究；Rig AgentRunner 的适配评估见 poc/rig-core 分支
//! （MSRV≥1.88、默认 Responses API、同步桥接成本，暂不引入）。

/// 一个 Agent 的声明式配置（与 `agent_profiles` 表一一对应）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    /// 稳定标识（如 `knowledge-analyst`）。
    pub name: String,
    /// 展示名（如「知识分析师」）。
    pub display_name: String,
    /// 模型覆盖；空串 = 跟随全局 AI 设置。
    pub model: String,
    /// 拥有的 Skill 名单（按声明顺序执行）。
    pub skills: Vec<String>,
    /// 权限上限（read / propose）。
    pub policy: Vec<String>,
    /// 预留：ReAct 型 Agent 的系统提示。
    pub system_prompt: String,
}
