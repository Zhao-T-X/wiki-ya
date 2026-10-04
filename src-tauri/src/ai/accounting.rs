//! Token 用量核算（PR-04：Real Token Accounting）。
//!
//! 背景：迁移到 Rig 之前，token 用量一直是「猜」的（按字符启发式估算），
//! 成本更是无从谈起。本模块把**真实用量**与**成本估算**收敛到一处：
//!
//! - [`TokenUsage`]：一次（或累计多次）调用的真实 token 账本——
//!   输入 / 输出 / embedding / 重试次数，全部来自 provider 返回的 `usage`，
//!   **绝不估算、绝不编造**。
//! - [`TokenUsage::estimate_cost_usd`]：基于价格表给出美元估算；
//!   价格表只覆盖已知模型，**未知模型返回 `None`（不假装能计价）**。

use serde::{Deserialize, Serialize};

/// 一次（或累计）调用的真实 token 账本。
///
/// 所有字段都来自 provider 的真实回报，缺省为 0（未启用 / 离线 / 未上报）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// 补全输入 token（prompt）。
    pub input_tokens: u64,
    /// 补全输出 token（completion）。
    pub output_tokens: u64,
    /// embedding 输入 token（向量化）。
    pub embedding_tokens: u64,
    /// 在首个成功尝试之外的额外重试次数（推理模型空体兜底等）。
    pub retries: u32,
}

impl TokenUsage {
    /// 由一次补全的真实用量构造。
    pub fn from_completion(input_tokens: u64, output_tokens: u64, retries: u32) -> Self {
        TokenUsage {
            input_tokens,
            output_tokens,
            embedding_tokens: 0,
            retries,
        }
    }

    /// 由一次 embedding 的真实用量构造。
    pub fn from_embedding(prompt_tokens: u64) -> Self {
        TokenUsage {
            embedding_tokens: prompt_tokens,
            ..Default::default()
        }
    }

    /// 累加另一笔用量（多次调用累计）。
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.embedding_tokens += other.embedding_tokens;
        self.retries += other.retries;
    }

    /// 补全类 token 总数（输入 + 输出）。
    pub fn completion_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// 全部 token 总数（补全 + embedding）。
    pub fn total_tokens(&self) -> u64 {
        self.completion_tokens() + self.embedding_tokens
    }

    /// 成本估算（美元）。
    ///
    /// 分别按 `model`（补全）与 `embedding_model`（向量化）查价格表；
    /// 任一部分能计价就计入，两部分都未知则返回 `None`（诚实：不编造价格）。
    pub fn estimate_cost_usd(&self, model: &str, embedding_model: &str) -> Option<f64> {
        let comp = price_per_1k(model).map(|(input, output)| {
            self.input_tokens as f64 / 1000.0 * input
                + self.output_tokens as f64 / 1000.0 * output
        });
        let emb = embed_price_per_1k(embedding_model)
            .map(|p| self.embedding_tokens as f64 / 1000.0 * p);
        match (comp, emb) {
            (None, None) => None,
            _ => Some(comp.unwrap_or(0.0) + emb.unwrap_or(0.0)),
        }
    }
}

/// 补全价格表：返回 `(输入价, 输出价)`，单位均为 **美元 / 1K tokens**。
///
/// 价格为公开标价的近似值（截至 2025 年），仅供估算；若你的供应商有折扣 /
/// 专属定价，请在此更新。未知模型返回 `None`。
fn price_per_1k(model: &str) -> Option<(f64, f64)> {
    let m = model.to_ascii_lowercase();
    let by_prefix = |prefix: &str| m.starts_with(prefix);
    // 精确匹配优先，其次前缀匹配（覆盖带日期后缀的模型名）。
    match m.as_str() {
        "gpt-4o" => Some((0.0025, 0.010)),
        "gpt-4o-mini" => Some((0.00015, 0.0006)),
        "gpt-4-turbo" | "gpt-4-turbo-preview" => Some((0.01, 0.03)),
        "gpt-4" => Some((0.03, 0.06)),
        "gpt-3.5-turbo" => Some((0.0005, 0.0015)),
        "deepseek-chat" => Some((0.00027, 0.0011)),
        "deepseek-reasoner" => Some((0.00055, 0.00219)),
        "claude-3-5-sonnet" | "claude-3.5-sonnet" => Some((0.003, 0.015)),
        _ if by_prefix("gpt-4o-mini") => Some((0.00015, 0.0006)),
        _ if by_prefix("gpt-4o") => Some((0.0025, 0.010)),
        _ if by_prefix("deepseek-chat") => Some((0.00027, 0.0011)),
        _ if by_prefix("deepseek-reasoner") => Some((0.00055, 0.00219)),
        _ => None,
    }
}

/// embedding 价格表：返回 **美元 / 1K tokens**。未知模型返回 `None`。
fn embed_price_per_1k(model: &str) -> Option<f64> {
    let m = model.to_ascii_lowercase();
    match m.as_str() {
        "text-embedding-3-small" => Some(0.00002),
        "text-embedding-3-large" => Some(0.00013),
        "text-embedding-ada-002" => Some(0.0001),
        _ if m.starts_with("text-embedding-3-small") => Some(0.00002),
        _ if m.starts_with("text-embedding-3-large") => Some(0.00013),
        _ if m.starts_with("text-embedding-ada-002") => Some(0.0001),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_add_accumulates() {
        let mut total = TokenUsage::default();
        total.add(&TokenUsage::from_completion(100, 20, 0));
        total.add(&TokenUsage::from_completion(50, 10, 1));
        total.add(&TokenUsage::from_embedding(300));
        assert_eq!(total.input_tokens, 150);
        assert_eq!(total.output_tokens, 30);
        assert_eq!(total.embedding_tokens, 300);
        assert_eq!(total.retries, 1);
        assert_eq!(total.completion_tokens(), 180);
        assert_eq!(total.total_tokens(), 480);
    }

    #[test]
    fn cost_estimates_known_models() {
        // gpt-4o-mini: in 0.00015 / out 0.0006（美元/1K）→ 1000+1000 = 0.00075
        let u = TokenUsage::from_completion(1000, 1000, 0);
        let cost = u.estimate_cost_usd("gpt-4o-mini", "text-embedding-3-small");
        assert!((cost.unwrap() - 0.00075).abs() < 1e-12);

        // embedding 部分：text-embedding-3-small 0.00002/1K × 5000 = 0.0001
        let u = TokenUsage::from_embedding(5000);
        let cost = u.estimate_cost_usd("gpt-4o-mini", "text-embedding-3-small");
        assert!((cost.unwrap() - 0.0001).abs() < 1e-12);
    }

    #[test]
    fn cost_unknown_model_is_none() {
        // 两部分都未知 → None（不编造价格）。
        let u = TokenUsage::from_completion(100, 10, 0);
        assert_eq!(u.estimate_cost_usd("some-unknown-model", "some-unknown-embed"), None);
        // 只有 embedding 已知 → 仍返回 Some（该部分可计价）。
        let u = TokenUsage::from_embedding(1000);
        assert!(u.estimate_cost_usd("some-unknown-model", "text-embedding-3-small").is_some());
    }
}
