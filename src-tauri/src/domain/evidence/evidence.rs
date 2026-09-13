//! 证据本身，以及「该展开到第几层」的确定性判定。

use crate::domain::common::ids::{ChunkId, ClaimId, DocumentId, EvidenceId};
use crate::domain::common::Timestamp;
use crate::domain::knowledge::document::SourceType;
use crate::error::{AppError, AppResult};

/// 证据分级（5 层）。
///
/// 层名与语义取自**参考实现的可运行实现**，而不是 PRD 的抽象命名
/// （决策 D5）：每一层都有真实的渲染分支与字符上限，
/// 而 PRD 的 `L1 Summary / L5 Original Source` 命名找不到对应代码路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceLevel {
    /// L1 只给引文本身——默认层，最省 Token。
    Quote = 1,
    /// L2 引文 + 单句窗口：条件式断言需要看到前提。
    QuoteContext = 2,
    /// L3 整个段落：出现矛盾证据时需要更多上下文。
    Paragraph = 3,
    /// L4 整个 chunk。
    Chunk = 4,
    /// L5 跨多个 chunk。默认不可达，必须显式提升。
    MultiChunk = 5,
}

impl EvidenceLevel {
    pub const ALL: &'static [EvidenceLevel] = &[
        EvidenceLevel::Quote,
        EvidenceLevel::QuoteContext,
        EvidenceLevel::Paragraph,
        EvidenceLevel::Chunk,
        EvidenceLevel::MultiChunk,
    ];

    pub const fn as_u8(&self) -> u8 {
        *self as u8
    }

    /// 展示名（IPC 契约里的 `evidenceLevelName`）。
    pub const fn name(&self) -> &'static str {
        match self {
            EvidenceLevel::Quote => "quote",
            EvidenceLevel::QuoteContext => "quote+context",
            EvidenceLevel::Paragraph => "paragraph",
            EvidenceLevel::Chunk => "chunk",
            EvidenceLevel::MultiChunk => "multi-chunk",
        }
    }

    /// 每层的字符上限。`None` 表示不额外截断。
    pub const fn char_cap(&self) -> Option<usize> {
        match self {
            EvidenceLevel::Quote => Some(400),
            EvidenceLevel::QuoteContext => Some(420),
            EvidenceLevel::Paragraph => Some(800),
            EvidenceLevel::Chunk | EvidenceLevel::MultiChunk => None,
        }
    }

    pub fn from_u8(value: u8) -> AppResult<EvidenceLevel> {
        EvidenceLevel::ALL
            .iter()
            .copied()
            .find(|level| level.as_u8() == value)
            .ok_or_else(|| {
                AppError::Domain(format!(
                    "证据层级必须是 1..=5，实际为 {value}"
                ))
            })
    }
}

impl std::fmt::Display for EvidenceLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "L{} {}", self.as_u8(), self.name())
    }
}

impl serde::Serialize for EvidenceLevel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.as_u8())
    }
}

impl<'de> serde::Deserialize<'de> for EvidenceLevel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u8::deserialize(deserializer)?;
        EvidenceLevel::from_u8(raw).map_err(serde::de::Error::custom)
    }
}

impl rusqlite::types::ToSql for EvidenceLevel {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(i64::from(self.as_u8())))
    }
}

impl rusqlite::types::FromSql for EvidenceLevel {
    fn column_result(
        value: rusqlite::types::ValueRef<'_>,
    ) -> rusqlite::types::FromSqlResult<Self> {
        let raw = value.as_i64()?;
        EvidenceLevel::from_u8(u8::try_from(raw).unwrap_or(0)).map_err(|err| {
            rusqlite::types::FromSqlError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )))
        })
    }
}

/// 默认展开上限：到 chunk 为止。
///
/// L5 需要显式请求，因为它是「把整段原文塞进上下文」，
/// 与 Context Efficiency 的目标直接冲突。
pub const DEFAULT_MAX_LEVEL: EvidenceLevel = EvidenceLevel::Chunk;

/// 低于该置信度就升到 L2（把条件/上下文一起给模型）。
pub const LOW_CONFIDENCE: f32 = 0.6;

/// 引文与原文的模糊匹配阈值。
pub const SIMILAR_QUOTE_RATIO: f32 = 0.85;

/// 短于该长度的字符串不做模糊匹配。
///
/// 一个字符的差异对长句子是笔误，对短字符串可能是完全不同的东西。
pub const MIN_FUZZY_CHARS: usize = 40;

/// 每个文档最多取几条证据，避免单文档淹没上下文。
pub const DEFAULT_PER_DOCUMENT_CAP: usize = 2;

/// 证据。
///
/// 注意 `quote` 是可空的：抽取可能拿不到精确引文，
/// 此时应退回段落级证据，而不是伪造一段引文。
#[derive(Debug, Clone)]
pub struct Evidence {
    pub id: EvidenceId,
    pub claim_id: ClaimId,
    pub document_id: DocumentId,
    pub chunk_id: Option<ChunkId>,
    pub start_offset: Option<usize>,
    pub end_offset: Option<usize>,
    pub quote: Option<String>,
    pub evidence_level: EvidenceLevel,
    pub source_type: SourceType,
    pub confidence: Option<f32>,
    pub created_at: Timestamp,
}

/// 升层原因（可解释，用于 Context Trace 与 UI 的「为什么？」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationDecision {
    pub level: EvidenceLevel,
    pub reason: String,
}

/// 选择「仍然站得住脚的最便宜的那一层」。
///
/// 触发条件只会**升层**，不会降层；原因记录最严重的那一条，
/// 因为用户问「为什么给我看这么多」时，他想知道的是最严重的那个理由。
pub fn choose_level(
    has_quote: bool,
    conditional: bool,
    best_confidence: Option<f32>,
    conflicts: bool,
    insufficient: bool,
    max_level: EvidenceLevel,
) -> EscalationDecision {
    // 没有引文时无法用最便宜的一层，直接给段落（且不超过上限）。
    if !has_quote {
        return EscalationDecision {
            level: clamp_level(EvidenceLevel::Paragraph, max_level),
            reason: "没有存储引文，退回所在段落".to_string(),
        };
    }

    let mut level = EvidenceLevel::Quote;
    let mut reason = "引文足以支撑".to_string();

    if insufficient {
        level = level.max(EvidenceLevel::QuoteContext);
        reason = "证据命中过少，需要额外上下文才能支撑结论".to_string();
    }
    if conditional {
        level = level.max(EvidenceLevel::QuoteContext);
        reason = "条件式断言，前提就在引文旁边".to_string();
    }
    if let Some(confidence) = best_confidence {
        if confidence < LOW_CONFIDENCE {
            level = level.max(EvidenceLevel::QuoteContext);
            reason = format!("置信度偏低（{confidence}）");
        }
    }
    if conflicts {
        level = level.max(EvidenceLevel::Paragraph);
        reason = "该问题存在相互矛盾的证据".to_string();
    }

    EscalationDecision {
        level: clamp_level(level, max_level),
        reason,
    }
}

/// 上限只降不升：`max_level` 是硬约束，任何触发都不能突破它。
fn clamp_level(level: EvidenceLevel, max_level: EvidenceLevel) -> EvidenceLevel {
    if level > max_level {
        max_level
    } else {
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_one_through_five_with_names() {
        assert_eq!(EvidenceLevel::ALL.len(), 5);
        assert_eq!(EvidenceLevel::Quote.as_u8(), 1);
        assert_eq!(EvidenceLevel::MultiChunk.as_u8(), 5);
        assert_eq!(EvidenceLevel::QuoteContext.name(), "quote+context");
        assert!(EvidenceLevel::from_u8(0).is_err());
        assert!(EvidenceLevel::from_u8(6).is_err());
    }

    #[test]
    fn a_good_quote_stays_at_the_cheapest_level() {
        let decision = choose_level(
            true,
            false,
            Some(0.9),
            false,
            false,
            DEFAULT_MAX_LEVEL,
        );
        assert_eq!(decision.level, EvidenceLevel::Quote);
    }

    #[test]
    fn triggers_only_escalate_and_record_the_most_serious_reason() {
        let decision = choose_level(
            true,
            true,
            Some(0.4),
            true,
            false,
            DEFAULT_MAX_LEVEL,
        );
        assert_eq!(decision.level, EvidenceLevel::Paragraph);
        assert!(decision.reason.contains("矛盾"));
    }

    #[test]
    fn max_level_always_wins() {
        let decision = choose_level(
            true,
            true,
            Some(0.1),
            true,
            true,
            EvidenceLevel::QuoteContext,
        );
        assert_eq!(decision.level, EvidenceLevel::QuoteContext);
    }

    #[test]
    fn missing_quote_falls_back_to_paragraph_not_to_a_fabricated_one() {
        let decision = choose_level(false, false, None, false, false, DEFAULT_MAX_LEVEL);
        assert_eq!(decision.level, EvidenceLevel::Paragraph);
        assert!(decision.reason.contains("引文"));
    }

    #[test]
    fn low_confidence_escalates_to_quote_context() {
        let decision = choose_level(true, false, Some(0.5), false, false, DEFAULT_MAX_LEVEL);
        assert_eq!(decision.level, EvidenceLevel::QuoteContext);
        assert_eq!(decision.reason, "置信度偏低（0.5）");
    }

    #[test]
    fn serde_and_sqlite_roundtrip() {
        let level = EvidenceLevel::Paragraph;
        assert_eq!(serde_json::to_string(&level).unwrap(), "3");
        assert_eq!(
            serde_json::from_str::<EvidenceLevel>("3").unwrap(),
            level
        );
    }
}
