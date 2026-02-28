//! Paragraph-boundary text chunker — re-exported from `context-harness-core`.
//!
//! # Example
//!
//! ```rust
//! use context_harness::chunk::chunk_text;
//!
//! let chunks = chunk_text("doc-123", "Hello world.\n\nSecond paragraph.", 700);
//! assert_eq!(chunks.len(), 1);
//! assert_eq!(chunks[0].chunk_index, 0);
//! ```

pub use context_harness_core::chunk::*;
