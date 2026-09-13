//! Chunk —— 文档的计算单位。
//!
//! 切分**完全确定性**、不调用任何模型：Local-first 的承诺要求
//! 「没有 API Key 也能 Capture 并建立可检索的结构」（PRD §44），
//! 因此切分必须在本地纯计算完成。
//!
//! ## 偏移量约定
//!
//! `start_offset` / `end_offset` 是原文的 **UTF-8 字节偏移**，
//! 切点保证落在字符边界上（因此不会把一个汉字劈成两半）。
//! 前端**不要**用这两个值去 slice JS 字符串（JS 是 UTF-16，索引体系不同）；
//! 它们用于定位、去重与证据回指。

use crate::domain::common::ids::{ChunkId, DocumentId};

/// 目标块大小（字符）。
const TARGET_CHUNK_CHARS: usize = 900;
/// 单块上限（字符）。超过就按句子边界强制切开。
const MAX_CHUNK_CHARS: usize = 1600;
/// 强制切分时的最小片段长度，避免切出大量碎片。
const MIN_SPLIT_CHARS: usize = 240;

/// 尚未落库的块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDraft {
    pub chunk_index: usize,
    pub start_offset: usize,
    pub end_offset: usize,
    pub content: String,
}

/// 已落库的块。
#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: ChunkId,
    pub document_id: DocumentId,
    pub chunk_index: usize,
    pub start_offset: usize,
    pub end_offset: usize,
    pub content: String,
    pub char_count: usize,
}

impl Chunk {
    /// 字符数（不是字节数）——中文文档用字节数会让「大小」显示得毫无意义。
    pub fn char_count_of(content: &str) -> usize {
        content.chars().count()
    }
}

/// 把文档正文切成块。
///
/// 策略：先按空行切段落，再把段落贪心合并到目标大小，
/// 超长段落按句子边界强制切分。
///
/// 为什么按段落而不是按固定长度：一个 chunk 会成为一个证据单位，
/// 从句子中间开始切出来的证据在 Review 界面里无法阅读，
/// 也就无法被人工判断真伪。
pub fn chunk_document(content: &str) -> Vec<ChunkDraft> {
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    for (start, end) in paragraph_spans(content) {
        if char_len(&content[start..end]) > MAX_CHUNK_CHARS {
            pieces.extend(split_oversized(content, start, end));
        } else {
            pieces.push((start, end));
        }
    }

    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in pieces {
        let fits = merged.last().is_some_and(|last| {
            char_len(&content[last.0..last.1]) + char_len(&content[start..end])
                <= TARGET_CHUNK_CHARS
        });
        if fits {
            if let Some(last) = merged.last_mut() {
                last.1 = end;
            }
        } else {
            merged.push((start, end));
        }
    }

    merged
        .into_iter()
        .filter(|(start, end)| !content[*start..*end].trim().is_empty())
        .enumerate()
        .map(|(index, (start, end))| ChunkDraft {
            chunk_index: index,
            start_offset: start,
            end_offset: end,
            content: content[start..end].to_string(),
        })
        .collect()
}

/// 段落区间，覆盖 `[0, len)`、互不重叠且连续。
fn paragraph_spans(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'\n' {
            // 判断这一行之后是否还有空行（允许行尾有空格/制表符）
            let mut probe = index + 1;
            while probe < bytes.len() && matches!(bytes[probe], b' ' | b'\t' | b'\r') {
                probe += 1;
            }
            if probe < bytes.len() && bytes[probe] == b'\n' {
                if index > start {
                    spans.push((start, index));
                }
                // 跳过全部连续空白与换行，下一段从第一个非空白字符开始
                let mut next = probe;
                while next < bytes.len()
                    && matches!(bytes[next], b'\n' | b' ' | b'\t' | b'\r')
                {
                    next += 1;
                }
                start = next;
                index = next;
                continue;
            }
        }
        index += 1;
    }

    if start < bytes.len() {
        spans.push((start, bytes.len()));
    }
    spans
}

/// 把一个超长段落按句子边界切开。
fn split_oversized(content: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut pieces = Vec::new();
    let mut piece_start = start;
    let mut last_break: Option<usize> = None;
    let mut since = 0usize;

    for (offset, ch) in content[start..end].char_indices() {
        let absolute_end = start + offset + ch.len_utf8();
        since += 1;

        if matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | '\n') {
            last_break = Some(absolute_end);
        }

        if since >= MAX_CHUNK_CHARS {
            let cut = match last_break {
                Some(break_at) if break_at > piece_start + MIN_SPLIT_CHARS => break_at,
                _ => absolute_end,
            };
            pieces.push((piece_start, cut));
            since = content[cut..absolute_end].chars().count();
            piece_start = cut;
            last_break = None;
        }
    }

    if piece_start < end {
        pieces.push((piece_start, end));
    }
    pieces
}

fn char_len(value: &str) -> usize {
    value.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_whitespace_documents_produce_no_chunks() {
        assert!(chunk_document("").is_empty());
        assert!(chunk_document("   \n\n  \n").is_empty());
    }

    #[test]
    fn short_document_becomes_a_single_chunk() {
        let chunks = chunk_document("Rust supports async fn in trait.");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].start_offset, 0);
        assert_eq!(chunks[0].content, "Rust supports async fn in trait.");
    }

    #[test]
    fn offsets_are_contiguous_and_mostly_cover_the_source() {
        let source = "First paragraph.\n\nSecond paragraph.\n\nThird one.";
        let chunks = chunk_document(source);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].start_offset, 0);
        // 相邻块的偏移必须单调递增，且每一块的切片都等于它自己的 content
        for pair in chunks.windows(2) {
            assert!(pair[0].end_offset <= pair[1].start_offset);
            assert_eq!(
                &source[pair[0].start_offset..pair[0].end_offset],
                pair[0].content
            );
        }
        let last = chunks.last().unwrap();
        assert_eq!(&source[last.start_offset..last.end_offset], last.content);
    }

    #[test]
    fn small_paragraphs_are_merged_up_to_the_target_size() {
        let paragraph = "A short paragraph about Rust.\n\n";
        let source = paragraph.repeat(20);
        let chunks = chunk_document(&source);
        assert!(
            chunks.len() < 20,
            "段落应被合并，实际产生 {} 块",
            chunks.len()
        );
        assert!(chunks
            .iter()
            .all(|c| c.content.chars().count() <= MAX_CHUNK_CHARS));
    }

    #[test]
    fn huge_paragraphs_are_split_at_sentence_boundaries() {
        let sentence = "这是一句关于知识演化的中文陈述。";
        let source = sentence.repeat(400); // 远超上限
        let chunks = chunk_document(&source);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.content.chars().count() <= MAX_CHUNK_CHARS,
                "块超长：{}",
                chunk.content.chars().count()
            );
            assert_eq!(
                &source[chunk.start_offset..chunk.end_offset],
                chunk.content
            );
        }
        // 切点应落在句末，而不是句中
        assert!(chunks[0].content.ends_with('。'));
    }

    #[test]
    fn multibyte_text_is_never_split_mid_character() {
        let source = "中文段落。".repeat(600);
        let chunks = chunk_document(&source);
        for chunk in &chunks {
            assert_eq!(
                &source[chunk.start_offset..chunk.end_offset],
                chunk.content
            );
        }
    }

    #[test]
    fn chunk_indexes_are_sequential_from_zero() {
        let source = "One.\n\nTwo.\n\nThree.".repeat(200);
        let chunks = chunk_document(&source);
        for (expected, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, expected);
        }
    }
}
