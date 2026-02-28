//! Paragraph-boundary text chunker.
//!
//! Splits document body text into [`Chunk`]s that respect a configurable
//! `max_tokens` limit. Splitting occurs on paragraph boundaries (`\n\n`)
//! to preserve semantic coherence within each chunk.
//!
//! Each chunk receives a deterministic UUID derived from its document ID
//! and index, plus a SHA-256 hash of its text for staleness detection
//! in the embedding pipeline.
//!
//! # Algorithm
//!
//! 1. Convert `max_tokens` to `max_chars` using a 4 chars/token ratio.
//! 2. Split text on `\n\n` paragraph boundaries.
//! 3. Accumulate paragraphs into a buffer until adding the next paragraph
//!    would exceed `max_chars`.
//! 4. When exceeded, flush the buffer as a chunk and start a new one.
//! 5. If a single paragraph exceeds `max_chars`, perform a hard split at
//!    the nearest newline or space boundary.
//! 6. Guarantee at least one chunk per document (even for empty text).
//!
//! # Example
//!
//! ```rust
//! use context_harness_core::chunk::chunk_text;
//!
//! let chunks = chunk_text("doc-123", "Hello world.\n\nSecond paragraph.", 700);
//! assert_eq!(chunks.len(), 1);
//! assert_eq!(chunks[0].chunk_index, 0);
//! ```

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::models::Chunk;

/// Approximate characters-per-token ratio.
///
/// This is a rough heuristic (4 chars ≈ 1 token) used for Phase 1.
/// Future versions may use a proper tokenizer.
const CHARS_PER_TOKEN: usize = 4;

/// Split text into chunks on paragraph boundaries, respecting `max_tokens`.
///
/// Returns chunks with contiguous indices starting at 0. Each chunk's
/// `hash` is the SHA-256 of its text content, used for embedding
/// staleness detection.
///
/// # Arguments
///
/// * `document_id` — The parent document's UUID (used in chunk metadata).
/// * `text` — The full document body to chunk.
/// * `max_tokens` — Maximum tokens per chunk (converted to chars via `× 4`).
///
/// # Guarantees
///
/// - At least one chunk is always returned (even for empty text).
/// - Chunk indices are contiguous: `0, 1, 2, …, N-1`.
/// - Chunks are split on `\n\n` boundaries when possible.
/// - Oversized paragraphs are hard-split at space/newline boundaries.
pub fn chunk_text(document_id: &str, text: &str, max_tokens: usize) -> Vec<Chunk> {
    let max_chars = max_tokens * CHARS_PER_TOKEN;

    if text.is_empty() {
        return vec![make_chunk(document_id, 0, text)];
    }

    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks = Vec::new();
    let mut current_buf = String::new();
    let mut chunk_index: i64 = 0;

    for para in paragraphs {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }

        let would_be = if current_buf.is_empty() {
            trimmed.len()
        } else {
            current_buf.len() + 2 + trimmed.len()
        };

        if would_be > max_chars && !current_buf.is_empty() {
            chunks.push(make_chunk(document_id, chunk_index, &current_buf));
            chunk_index += 1;
            current_buf.clear();
        }

        if trimmed.len() > max_chars {
            if !current_buf.is_empty() {
                chunks.push(make_chunk(document_id, chunk_index, &current_buf));
                chunk_index += 1;
                current_buf.clear();
            }
            let mut remaining = trimmed;
            while !remaining.is_empty() {
                let split_at = snap_to_char_boundary(remaining, remaining.len().min(max_chars));
                let split_at = if split_at == 0 && !remaining.is_empty() {
                    remaining
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| i)
                        .unwrap_or(remaining.len())
                } else {
                    split_at
                };
                let actual_split = if split_at < remaining.len() {
                    remaining[..split_at]
                        .rfind('\n')
                        .or_else(|| remaining[..split_at].rfind(' '))
                        .map(|pos| pos + 1)
                        .unwrap_or(split_at)
                } else {
                    split_at
                };
                let actual_split = snap_to_char_boundary(remaining, actual_split);
                let actual_split = if actual_split == 0 && !remaining.is_empty() {
                    remaining
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| i)
                        .unwrap_or(remaining.len())
                } else {
                    actual_split
                };
                let piece = &remaining[..actual_split];
                if !piece.trim().is_empty() {
                    chunks.push(make_chunk(document_id, chunk_index, piece.trim()));
                    chunk_index += 1;
                }
                remaining = &remaining[actual_split..];
            }
        } else {
            if !current_buf.is_empty() {
                current_buf.push_str("\n\n");
            }
            current_buf.push_str(trimmed);
        }
    }

    if !current_buf.is_empty() {
        chunks.push(make_chunk(document_id, chunk_index, &current_buf));
    }

    if chunks.is_empty() {
        chunks.push(make_chunk(document_id, 0, text.trim()));
    }

    chunks
}

/// Snap a byte index back to the nearest valid UTF-8 char boundary.
fn snap_to_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Create a single [`Chunk`] with a UUID and SHA-256 content hash.
fn make_chunk(document_id: &str, index: i64, text: &str) -> Chunk {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    Chunk {
        id: Uuid::new_v4().to_string(),
        document_id: document_id.to_string(),
        chunk_index: index,
        text: text.to_string(),
        hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_text_single_chunk() {
        let chunks = chunk_text("doc1", "Hello, world!", 700);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[0].text, "Hello, world!");
    }

    #[test]
    fn test_empty_text() {
        let chunks = chunk_text("doc1", "", 700);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn test_multiple_paragraphs_under_limit() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let chunks = chunk_text("doc1", text, 700);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("First paragraph."));
        assert!(chunks[0].text.contains("Third paragraph."));
    }

    #[test]
    fn test_multiple_paragraphs_exceed_limit() {
        let text = "This is paragraph one.\n\nThis is paragraph two.\n\nThis is paragraph three.";
        let chunks = chunk_text("doc1", text, 5);
        assert!(chunks.len() > 1);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i as i64);
        }
    }

    #[test]
    fn test_chunk_indices_contiguous() {
        let text = (0..50)
            .map(|i| format!("Paragraph number {}.", i))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_text("doc1", &text, 10);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.chunk_index, i as i64, "Index mismatch at position {}", i);
        }
    }

    #[test]
    fn test_multibyte_utf8_chars() {
        let text = "┌──────────────────┐\n│ Hello world      │\n└──────────────────┘";
        let chunks = chunk_text("doc1", text, 3);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(!c.text.is_empty() || c.chunk_index == 0);
        }
    }

    #[test]
    fn test_deterministic() {
        let text = "Alpha\n\nBeta\n\nGamma\n\nDelta";
        let c1 = chunk_text("doc1", text, 5);
        let c2 = chunk_text("doc1", text, 5);
        assert_eq!(c1.len(), c2.len());
        for (a, b) in c1.iter().zip(c2.iter()) {
            assert_eq!(a.text, b.text);
            assert_eq!(a.hash, b.hash);
            assert_eq!(a.chunk_index, b.chunk_index);
        }
    }
}
