//! 面向嵌入模型的文本分段（PERF-07 / 方案 A）。
//!
//! 背景：`bge-small-zh-v1.5` 的上下文是 **512 token**，而本库实测 73 个 chunk
//! 中有 **41 个（56%）超过该上限**，最长约 831 token。直接送入会被 tokenizer
//! **静默截断**——尾部内容永远搜不到。
//!
//! 做法：超限的 chunk 滑动切段，**每段各出一个向量**；检索时按 chunk 去重、
//! 取分数最高的那一段。短 chunk 行为完全不变（单段，与改造前等价）。
//!
//! ## 关于 token 计数（重要且诚实）
//!
//! 本模块**不依赖任何 tokenizer**，用**保守估计**：CJK 字符按 1 token 计
//! （BERT 中文模型的上界，最保守），ASCII 按 3 字符 1 token 计（真实约 4，
//! 略微高估）。刻意**高估**：宁可段切小一点、多切几段，也绝不让内容溢出
//! 模型窗口——溢出等于静默丢内容，正是我们要消灭的问题。代价是英文密集段落
//! 会被多切（中文语料下误差约 +30%，可接受）。
//! 接入本地运行时（fastembed-rs）后应改用真实 tokenizer，届时本估计仅作回退。

/// CJK 字符记 3 个单位，ASCII 字符记 1 个单位 → 3 个 ASCII = 1 token。
const UNITS_PER_TOKEN: usize = 3;

fn units_of(ch: char) -> usize {
    if is_cjk(ch) {
        UNITS_PER_TOKEN
    } else {
        1
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF    // 日文假名
        | 0x3400..=0x4DBF  // 扩展 A
        | 0x4E00..=0x9FFF  // 基本汉字
        | 0xF900..=0xFAFF  // 兼容汉字
        | 0xAC00..=0xD7AF  // 韩文
    )
}

/// 保守的 token 估计：CJK 1 token/字，ASCII 1 token/3 字符。
pub fn estimate_tokens(text: &str) -> usize {
    let units: usize = text.chars().map(units_of).sum();
    units.div_ceil(UNITS_PER_TOKEN)
}

/// 切出来的一段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 实际送去嵌入的文本。**必须原样存储**——检索返回的证据只能是它，
    /// 不能拿整个 chunk 冒充（那是虚报证据范围）。
    pub text: String,
    /// 该段在原文中的起始字符下标。
    pub char_start: usize,
    /// 该段在原文中的结束字符下标（不含）。
    pub char_end: usize,
    /// 估算 token 数（保守上界）。
    pub est_tokens: usize,
}

/// 把文本切成若干「每段不超过 `max_tokens`」的窗口，相邻重叠 `overlap_tokens`。
///
/// 文本本身就短于上限时返回**单段**——与改造前完全等价。
pub fn split_for_embedding(text: &str, max_tokens: usize, overlap_tokens: usize) -> Vec<Segment> {
    let max_tokens = max_tokens.max(1);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![Segment {
            text: String::new(),
            char_start: 0,
            char_end: 0,
            est_tokens: 0,
        }];
    }

    let total_units: usize = chars.iter().map(|c| units_of(*c)).sum();
    let max_units = max_tokens * UNITS_PER_TOKEN;
    if total_units <= max_units {
        return vec![Segment {
            text: text.to_string(),
            char_start: 0,
            char_end: chars.len(),
            est_tokens: total_units.div_ceil(UNITS_PER_TOKEN),
        }];
    }

    let overlap_units = overlap_tokens.min(max_tokens.saturating_sub(1)) * UNITS_PER_TOKEN;
    let mut segments: Vec<Segment> = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        // greedy：从 start 装到接近 max_units 为止
        let mut units = 0usize;
        let mut end = start;
        while end < chars.len() {
            let u = units_of(chars[end]);
            if units + u > max_units && end > start {
                break;
            }
            units += u;
            end += 1;
        }
        if end <= start {
            end = start + 1; // 兜底：至少推进一个字符，杜绝死循环
        }
        let seg_text: String = chars[start..end].iter().collect();
        segments.push(Segment {
            est_tokens: units.div_ceil(UNITS_PER_TOKEN),
            text: seg_text,
            char_start: start,
            char_end: end,
        });
        if end >= chars.len() {
            break;
        }
        // 回退 overlap 个单位（按字符向前累积），避免句子被拦腰截断
        let mut back_units = 0usize;
        let mut back = 0usize;
        while back < end - start && back_units < overlap_units {
            back_units += units_of(chars[end - 1 - back]);
            back += 1;
        }
        // 必须保证起点严格前进，否则会死循环（曾因 `.max(1)` 把起点拉回而挂死）。
        let next_start = end.saturating_sub(back);
        start = if next_start > start { next_start } else { end };
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_is_conservative_for_chinese() {
        // 纯中文：1 字 ≈ 1 token
        assert_eq!(estimate_tokens("知识契约"), 4);
        // 纯 ASCII：3 字符 ≈ 1 token
        assert_eq!(estimate_tokens("abcdef"), 2);
        assert_eq!(estimate_tokens("abc"), 1);
    }

    #[test]
    fn short_text_yields_exactly_one_segment() {
        let segs = split_for_embedding("短文本", 512, 64);
        assert_eq!(segs.len(), 1, "短文本必须只切一段（行为与改造前一致）");
        assert_eq!(segs[0].text, "短文本");
        assert_eq!(segs[0].char_start, 0);
        assert_eq!(segs[0].char_end, 3);
    }

    /// 核心不变量：**任何段都不得超限**——超限即等价于静默截断。
    #[test]
    fn no_segment_exceeds_the_limit() {
        let long = "知识契约与演化规范".repeat(400); // 4800 字 ≈ 4800 token
        for max_tokens in [128usize, 512, 1024] {
            for seg in split_for_embedding(&long, max_tokens, 64) {
                assert!(
                    seg.est_tokens <= max_tokens,
                    "段超出上限：est={} max={max_tokens}",
                    seg.est_tokens
                );
            }
        }
    }

    /// 覆盖性：拼接所有段（去掉重叠）后必须能还原全文——不允许丢内容。
    #[test]
    fn segments_cover_the_whole_text() {
        let long: String = (0..300).map(|i| format!("句子{i}。")).collect();
        let segs = split_for_embedding(&long, 64, 16);
        assert!(segs.len() > 1, "长文本应被切成多段");
        // 首段从 0 开始，末段到结尾，中途用重叠衔接
        assert_eq!(segs.first().unwrap().char_start, 0);
        assert_eq!(segs.last().unwrap().char_end, long.chars().count());
        for pair in segs.windows(2) {
            assert!(
                pair[0].char_end > pair[1].char_start,
                "相邻段必须有重叠，否则会丢内容"
            );
        }
    }

    #[test]
    fn empty_text_is_safe() {
        let segs = split_for_embedding("", 512, 64);
        assert_eq!(segs.len(), 1);
        assert!(segs[0].text.is_empty());
    }

    /// 单字符超长字符不会死循环。
    #[test]
    fn degenerate_limit_still_terminates() {
        let segs = split_for_embedding("一二三四五六七八九十", 1, 0);
        assert!(segs.len() >= 10, "max_tokens=1 时应逐字成段");
        assert!(segs.iter().all(|s| s.est_tokens <= 1));
    }
}
