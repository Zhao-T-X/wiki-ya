//! Context Efficiency Engine（Phase 6，最小可用集 + 增强）。
//!
//! 设计目标（TDD §33-§37）：在把上下文喂给 LLM 之前，做去重、优先级排序、
//! 预算裁剪，并产出透明可审计的 `ContextStats`（供 Ask 的「Context Stats」面板）。
//!
//! 本模块**不依赖数据库，也不依赖 Provider**，只做纯函数式的上下文编排，
//! 因此可被独立测试，也便于后续由 context-engine 成员扩展 planner / compression。
//!
//! 本次增强（仍保持 `compile` 的契约不变）：
//! - `estimate_tokens`：更贴近真实分词的中文/英文启发式，避免对中文严重低估。
//! - `planner`：依据长度/优先级为每条设定 `strategy`（信息字段）。
//! - `compression`：对超长条目在装桶前截断内容并重算 `token_cost`，并据以计算
//!   `stats.compression_ratio`（无压缩时恒为 1.0）。

use serde::{Deserialize, Serialize};

/// 上下文条目的来源类型（与检索结果、SearchKind 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Document,
    Chunk,
    Claim,
    Entity,
    Evidence,
}

impl ContextKind {
    pub fn from_search_kind(kind: &str) -> Option<Self> {
        match kind {
            "document" => Some(ContextKind::Document),
            "chunk" => Some(ContextKind::Chunk),
            "claim" => Some(ContextKind::Claim),
            "entity" => Some(ContextKind::Entity),
            _ => None,
        }
    }
}

/// 加载策略（TDD §35）：决定一条上下文要不要进模型、要不要先摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadStrategy {
    Load,
    Summarize,
    RetrieveLater,
    NeverLoad,
}

/// 一条上下文候选（TDD §34）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: String,
    pub kind: ContextKind,
    pub content: String,
    pub token_cost: usize,
    pub priority: f32,
    pub strategy: LoadStrategy,
    pub source_id: Option<String>,
    pub title: Option<String>,
}

/// 上下文预算：模型上下文窗口里能留给检索内容的上限。
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub max_tokens: usize,
}

/// 编译后的上下文包，直接拼进 prompt。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    /// 实际入选、会进入 prompt 的条目（已按优先级排序）。
    pub items: Vec<ContextItem>,
    pub total_tokens: usize,
    /// 是否因超出预算而被截断（UI 要如实告知，不能假装全量）。
    pub truncated: bool,
    pub stats: ContextStats,
}

/// 透明的上下文统计（供 Ask 的「Context Stats」面板审计）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStats {
    pub total_tokens: usize,
    pub loaded_tokens: usize,
    pub item_count: usize,
    pub truncated: bool,
    /// 压缩比：原始 token / 加载 token（无压缩时为 1.0）。
    pub compression_ratio: f32,
}

/// 判断一个字符是否属于 CJK（中日韩统一表意文字，及假名、谚文等）。
fn is_cjk(ch: char) -> bool {
    let c = ch as u32;
    (0x3400..=0x4DBF).contains(&c) // CJK Extension A
        || (0x4E00..=0x9FFF).contains(&c) // CJK Unified Ideographs
        || (0x3040..=0x30FF).contains(&c) // Hiragana + Katakana
        || (0xAC00..=0xD7AF).contains(&c) // Hangul Syllables
}

/// 粗略的 token 估算（仅用于预算，不追求精确）。
///
/// 启发式：
/// - CJK 字符：约 1.5 token/字符（中文一个字往往对应一个 token）。
/// - ASCII 单词：约 1.3 token/词（按空白/标点切词，不按字符）。
/// - 其它非空白字符（标点等）：轻量 ~0.3 token。
///
/// 这样对中文不再严重低估（旧公式 4 字符/token 会把中文少算约 6 倍）。
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0.0;
    let mut in_word = false;
    let mut cjk_count = 0usize;
    for ch in text.chars() {
        if is_cjk(ch) {
            cjk_count += 1;
            in_word = false;
        } else if ch.is_ascii_alphanumeric() {
            // 按「词」计费：每进入一个新单词记 ~1.3 token。
            if !in_word {
                tokens += 1.3;
                in_word = true;
            }
        } else {
            in_word = false;
            // 空白不收费；其它标点等轻量收费。
            if !ch.is_whitespace() {
                tokens += 0.3;
            }
        }
    }
    tokens += cjk_count as f32 * 1.5;
    (tokens.ceil() as usize).max(1)
}

/// Planner：依据长度/优先级为每条上下文设定加载策略。
///
/// 策略仅作为信息字段，真正的「裁剪」由 `compression` + `compile` 的预算逻辑完成，
/// 因此 planner 不改变 `compile` 的装入契约。
pub fn plan(items: &[ContextItem]) -> Vec<ContextItem> {
    items
        .iter()
        .map(|item| {
            let mut item = item.clone();
            item.strategy = decide_strategy(&item);
            item
        })
        .collect()
}

/// 依据单条条目的长度与优先级决定其加载策略。
fn decide_strategy(item: &ContextItem) -> LoadStrategy {
    const LONG_ITEM_TOKENS: usize = 2000;
    const SHORT_ITEM_TOKENS: usize = 100;
    const LOW_PRIORITY: f32 = 0.2;

    if item.token_cost >= LONG_ITEM_TOKENS {
        // 长条目：建议先摘要，避免占满窗口。
        LoadStrategy::Summarize
    } else if item.token_cost <= SHORT_ITEM_TOKENS && item.priority < LOW_PRIORITY {
        // 又短又低优先级：延后按需检索，先不占预算。
        LoadStrategy::RetrieveLater
    } else {
        LoadStrategy::Load
    }
}

/// Compression：当单条 `token_cost` 超过 `per_item_cap` 时，将其 `content` 截断到
/// 约 `cap` token 长度并重算 `token_cost`，返回压缩后的新条目；未超限则原样返回。
///
/// 注：受锁定的 `ContextItem` 结构约束，「是否被压缩」不另设字段，而是由调用方
/// （`compile`）比对原始/压缩后的 `token_cost` 来推导（见 `stats.compression_ratio`）。
pub fn compress(item: &ContextItem, per_item_cap: usize) -> ContextItem {
    if item.token_cost <= per_item_cap {
        return item.clone();
    }
    let chars: Vec<char> = item.content.chars().collect();
    if chars.is_empty() || item.token_cost == 0 {
        return item.clone();
    }

    // 按 token 占比估算应保留的字符数；若仍超 cap，则按 0.85 安全系数逐步缩减
    // （至少保留 1 个字符），确保压缩后确实低于 cap。
    let ratio = per_item_cap as f32 / item.token_cost as f32;
    let mut take = ((chars.len() as f32 * ratio) as usize).max(1);
    let mut out = item.clone();
    loop {
        let truncated: String = chars.iter().take(take).collect();
        if take <= 1 || estimate_tokens(&truncated) <= per_item_cap {
            out.content = truncated;
            break;
        }
        take = (take as f32 * 0.85) as usize;
    }
    out.token_cost = estimate_tokens(&out.content);
    out
}

/// 编译上下文：plan → compress → 去重 → 按优先级排序 → 预算裁剪。
///
/// 这是「诚实优先」的关键：超出预算的条目直接丢弃并标记 `truncated`，
/// 绝不明知超窗口还硬塞（那会静默截断、产生幻觉风险）。
///
/// 公开签名由主干锁定；内部已串联 planner / compression，但不得改变 `compile`
/// 的入参、返回结构与语义（去重保留首次、按 priority 降序、贪心装桶、至少一条）。
pub fn compile(mut items: Vec<ContextItem>, budget: &Budget) -> ContextPack {
    // 1) Planner：为每条设定加载策略（信息字段）。
    let planned = plan(&items);

    // 2) Compression：对超长条目在装桶前先截断，避免单条撑爆预算。
    //    单条上限取预算的 1/4（且恒不超过预算本身），确保压缩后必然落入窗口。
    let per_item_cap = (budget.max_tokens / 4).max(1);
    let mut any_compressed = false;
    let mut orig_tokens = 0usize;
    let mut post_tokens = 0usize;
    let mut compressed: Vec<ContextItem> = Vec::with_capacity(planned.len());
    for mut item in planned {
        let original = item.token_cost;
        if original > per_item_cap {
            let c = compress(&item, per_item_cap);
            any_compressed = true;
            orig_tokens += original;
            post_tokens += c.token_cost;
            item = c;
        }
        compressed.push(item);
    }
    items = compressed;

    // 3) 去重：同 id 只保留一条（首次出现优先），保持顺序稳定。
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.id.clone()));

    // 4) 按优先级降序；同优先级保持原顺序。
    items.sort_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap_or(std::cmp::Ordering::Equal));

    // 5) 贪心装入直到超出预算（至少保留一条，避免有内容却空包）。
    let mut selected = Vec::new();
    let mut total = 0usize;
    let mut truncated = false;
    for item in items {
        let cost = item.token_cost;
        if total + cost > budget.max_tokens && !selected.is_empty() {
            truncated = true;
            break;
        }
        total += cost;
        selected.push(item);
    }

    let item_count = selected.len();
    let compression_ratio = if any_compressed && post_tokens > 0 {
        orig_tokens as f32 / post_tokens as f32
    } else {
        1.0
    };

    ContextPack {
        items: selected,
        total_tokens: total,
        truncated,
        stats: ContextStats {
            total_tokens: total,
            loaded_tokens: total,
            item_count,
            truncated,
            compression_ratio,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, content: &str, cost: usize, priority: f32) -> ContextItem {
        ContextItem {
            id: id.to_string(),
            kind: ContextKind::Chunk,
            content: content.to_string(),
            token_cost: cost,
            priority,
            strategy: LoadStrategy::Load,
            source_id: None,
            title: None,
        }
    }

    #[test]
    fn test_estimate_tokens_cjk_not_underestimated() {
        // 4 个中文字 ≈ 6 token，旧公式（4 字符/token）只会给出 1。
        let t = estimate_tokens("中文测试");
        assert!(t >= 4, "中文被严重低估: {t}");
    }

    #[test]
    fn test_estimate_tokens_ascii_word_aware() {
        let t = estimate_tokens("the quick brown fox");
        // 4 个词 ≈ 5.2 token；应明显大于纯字符计数预期的下界。
        assert!(t >= 4, "英文单词被低估: {t}");
        assert!(t < 20, "英文单词被高估: {t}");
    }

    #[test]
    fn test_estimate_tokens_empty_is_one() {
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("   "), 1);
    }

    #[test]
    fn test_dedupe_keeps_first_occurrence() {
        let items = vec![
            item("a", "first", 10, 1.0),
            item("a", "dup", 10, 0.5),
            item("b", "second", 10, 1.0),
        ];
        let pack = compile(items, &Budget { max_tokens: 1000 });
        assert_eq!(pack.items.len(), 2);
        assert_eq!(pack.items[0].id, "a");
        assert_eq!(pack.items[0].content, "first"); // 保留首次出现
    }

    #[test]
    fn test_budget_truncation_marks_truncated() {
        // 6 条大条目；每条经压缩后仍约 ~250 token（cap = 预算 1/4 = 250）。
        // 预算 1000 最多装下 4 条，其余因超预算被丢弃并标记 truncated。
        let content = "word ".repeat(300); // ≈ 390 token
        let items: Vec<ContextItem> = (0..6)
            .map(|i| item(&format!("k{i}"), &content, 390, 1.0 - i as f32 * 0.01))
            .collect();
        let pack = compile(items, &Budget { max_tokens: 1000 });
        assert!(pack.truncated, "应因超预算被截断");
        assert!(pack.items.len() < 6, "应有条目被丢弃, got {}", pack.items.len());
        assert!(pack.total_tokens <= 1000, "不应超预算, got {}", pack.total_tokens);
        assert!(pack.stats.truncated);
        assert_eq!(pack.stats.item_count, pack.items.len());
    }

    #[test]
    fn test_budget_keeps_at_least_one_item() {
        let items = vec![item("a", "big", 1000, 1.0)];
        let pack = compile(items, &Budget { max_tokens: 100 });
        // 即便单条原始超预算，也至少保留一条；且经压缩后（cap=25）落入预算，不硬塞超预算内容。
        assert_eq!(pack.items.len(), 1);
        assert!(pack.total_tokens <= 100, "压缩后仍超预算: {}", pack.total_tokens);
    }

    #[test]
    fn test_priority_descending_order() {
        let items = vec![
            item("a", "x", 10, 0.2),
            item("b", "y", 10, 0.9),
            item("c", "z", 10, 0.5),
        ];
        let pack = compile(items, &Budget { max_tokens: 1000 });
        let prios: Vec<f32> = pack.items.iter().map(|i| i.priority).collect();
        assert_eq!(prios, vec![0.9, 0.5, 0.2]);
    }

    #[test]
    fn test_compress_truncates_when_over_cap() {
        let big = "word ".repeat(600); // 约 600 词 ≈ 780 token
        let it = item("a", &big, 780, 1.0);
        let c = compress(&it, 250);
        assert!(c.token_cost <= 250, "压缩后仍超 cap: {}", c.token_cost);
        assert!(c.content.len() < it.content.len());
    }

    #[test]
    fn test_compress_noop_when_under_cap() {
        let it = item("a", "short", 50, 1.0);
        let c = compress(&it, 250);
        assert_eq!(c.token_cost, it.token_cost);
        assert_eq!(c.content, it.content);
    }

    #[test]
    fn test_compression_in_compile_sets_ratio() {
        let big = "word ".repeat(600); // ≈ 780 token
        let items = vec![item("a", &big, 780, 1.0)];
        let pack = compile(items, &Budget { max_tokens: 1000 });
        // 预算 1000 → per_item_cap 250，单条被压缩且装入。
        assert!(pack.items[0].token_cost <= 250);
        assert!(pack.total_tokens <= 250);
        assert!(
            pack.stats.compression_ratio > 1.0,
            "期望发生压缩, ratio={}",
            pack.stats.compression_ratio
        );
    }

    #[test]
    fn test_no_compression_ratio_is_one() {
        let items = vec![item("a", "x", 10, 1.0), item("b", "y", 10, 0.5)];
        let pack = compile(items, &Budget { max_tokens: 1000 });
        assert_eq!(pack.stats.compression_ratio, 1.0);
        assert!(!pack.truncated);
    }

    #[test]
    fn test_planner_strategy_assignment() {
        let long = item("a", "x", 3000, 0.9);
        let short_low = item("b", "y", 50, 0.1);
        let normal = item("c", "z", 500, 0.8);
        let planned = plan(&[long, short_low, normal]);
        assert_eq!(planned[0].strategy, LoadStrategy::Summarize);
        assert_eq!(planned[1].strategy, LoadStrategy::RetrieveLater);
        assert_eq!(planned[2].strategy, LoadStrategy::Load);
    }
}
