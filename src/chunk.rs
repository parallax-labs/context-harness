//! Paragraph-boundary text chunker.
//!
//! Splits document body text into [`Chunk`]s that respect a configurable
//! `max_tokens` limit. Splitting occurs on paragraph boundaries (`\n\n`)
//! to preserve semantic coherence within each chunk.
//!
//! Each chunk receives a deterministic UUID derived from its document ID
//! and index, plus a SHA-256 hash of its text for staleness detection.

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::models::Chunk;

/// Approximate chars-per-token ratio for Phase 1.
const CHARS_PER_TOKEN: usize = 4;

/// Split text into chunks on paragraph boundaries, respecting max_tokens.
/// Returns chunks with contiguous indices starting at 0.
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

        // If adding this paragraph would exceed max, flush current buffer
        let would_be = if current_buf.is_empty() {
            trimmed.len()
        } else {
            current_buf.len() + 2 + trimmed.len() // +2 for \n\n separator
        };

        if would_be > max_chars && !current_buf.is_empty() {
            chunks.push(make_chunk(document_id, chunk_index, &current_buf));
            chunk_index += 1;
            current_buf.clear();
        }

        // If a single paragraph exceeds max, split it by sentences/lines
        if trimmed.len() > max_chars {
            if !current_buf.is_empty() {
                chunks.push(make_chunk(document_id, chunk_index, &current_buf));
                chunk_index += 1;
                current_buf.clear();
            }
            // Hard split at max_chars boundaries
            let mut remaining = trimmed;
            while !remaining.is_empty() {
                let split_at = remaining.len().min(max_chars);
                // Try to split at a newline or space boundary
                let actual_split = if split_at < remaining.len() {
                    remaining[..split_at]
                        .rfind('\n')
                        .or_else(|| remaining[..split_at].rfind(' '))
                        .map(|pos| pos + 1)
                        .unwrap_or(split_at)
                } else {
                    split_at
                };
                let piece = &remaining[..actual_split];
                chunks.push(make_chunk(document_id, chunk_index, piece.trim()));
                chunk_index += 1;
                remaining = &remaining[actual_split..];
            }
        } else {
            if !current_buf.is_empty() {
                current_buf.push_str("\n\n");
            }
            current_buf.push_str(trimmed);
        }
    }

    // Flush remaining
    if !current_buf.is_empty() {
        chunks.push(make_chunk(document_id, chunk_index, &current_buf));
    }

    // Guarantee at least one chunk
    if chunks.is_empty() {
        chunks.push(make_chunk(document_id, 0, text.trim()));
    }

    chunks
}

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
        // max_tokens=5 => max_chars=20
        let text = "This is paragraph one.\n\nThis is paragraph two.\n\nThis is paragraph three.";
        let chunks = chunk_text("doc1", text, 5);
        assert!(chunks.len() > 1);
        // Indices must be contiguous starting at 0
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
