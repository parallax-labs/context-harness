//! Core data models used throughout Context Harness.
//!
//! These types represent the documents, chunks, and search results that flow
//! through the ingestion and retrieval pipeline. The data lifecycle is:
//!
//! ```text
//! Connector → SourceItem → normalize() → Document → chunk() → Chunk
//!                                                       ↓
//!                                                  embed() → Embedding
//!                                                       ↓
//!                                                  search() → SearchResult
//! ```
//!
//! # Type Relationships
//!
//! - A **[`SourceItem`]** is produced by a connector (filesystem, Git, S3)
//!   before any normalization or storage.
//! - A **[`Document`]** is the normalized, stored representation with a
//!   deduplication hash and Unix timestamps.
//! - A **[`Chunk`]** is a segment of a document's body, stored alongside
//!   a content hash for embedding staleness detection.
//! - A **[`SearchResult`]** is returned by the query engine with a
//!   relevance score and snippet.

use chrono::{DateTime, Utc};

/// Raw item produced by a connector before normalization.
///
/// Connectors (filesystem, Git, S3) emit `SourceItem`s that are then
/// normalized into [`Document`]s during the ingestion pipeline.
///
/// # Fields
///
/// | Field | Description |
/// |-------|-------------|
/// | `source` | Connector name, e.g. `"filesystem"`, `"git"`, `"s3"` |
/// | `source_id` | Unique identifier within the source (e.g. relative file path, S3 key) |
/// | `source_url` | Optional web-browsable URL (e.g. GitHub blob URL, `s3://` URI) |
/// | `title` | Human-readable title, typically the filename |
/// | `author` | Author extracted from source metadata (e.g. last Git committer) |
/// | `created_at` / `updated_at` | Timestamps from the source (commit time, mtime, S3 `LastModified`) |
/// | `content_type` | MIME type, e.g. `"text/plain"`, `"text/markdown"` |
/// | `body` | Full text content of the document |
/// | `metadata_json` | Connector-specific metadata as a JSON string |
/// | `raw_json` | Optional raw API response for debugging |
/// | `raw_bytes` | When set, the pipeline runs extraction and sets `body` before upsert; content_type identifies the format |
#[derive(Debug, Clone)]
pub struct SourceItem {
    /// Connector name: `"filesystem"`, `"git"`, or `"s3"`.
    pub source: String,
    /// Unique identifier within the source (e.g. relative file path or S3 object key).
    pub source_id: String,
    /// Web-browsable URL for the source item, if available.
    pub source_url: Option<String>,
    /// Human-readable title (typically the filename).
    pub title: Option<String>,
    /// Author extracted from source metadata (e.g. last Git committer).
    pub author: Option<String>,
    /// Creation timestamp from the source.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp from the source.
    pub updated_at: DateTime<Utc>,
    /// MIME content type (e.g. `"text/plain"`, `"text/markdown"`).
    pub content_type: String,
    /// Full text content of the document.
    pub body: String,
    /// Connector-specific metadata serialized as JSON.
    pub metadata_json: String,
    /// Optional raw API/connector response for debugging.
    pub raw_json: Option<String>,
    /// When set, the pipeline runs extraction and sets body from the result before upsert; content_type identifies the format.
    pub raw_bytes: Option<Vec<u8>>,
}

/// Normalized document stored in the SQLite `documents` table.
///
/// Created during ingestion by normalizing a [`SourceItem`]. Each document
/// is uniquely identified by the `(source, source_id)` pair, and carries
/// a `dedup_hash` (SHA-256 of source + source_id + updated_at + body) to
/// detect content changes.
///
/// Timestamps are stored as Unix epoch seconds (i64) for efficient
/// comparison and indexing.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Document {
    /// UUID v4 primary key.
    pub id: String,
    /// Connector name that produced this document.
    pub source: String,
    /// Unique identifier within the source.
    pub source_id: String,
    /// Web-browsable URL, if available.
    pub source_url: Option<String>,
    /// Human-readable title.
    pub title: Option<String>,
    /// Author from source metadata.
    pub author: Option<String>,
    /// Creation timestamp (Unix epoch seconds).
    pub created_at: i64,
    /// Last modification timestamp (Unix epoch seconds).
    pub updated_at: i64,
    /// MIME content type.
    pub content_type: String,
    /// Full text body.
    pub body: String,
    /// Connector-specific metadata as JSON.
    pub metadata_json: String,
    /// Raw connector response.
    pub raw_json: Option<String>,
    /// SHA-256 hash for deduplication: `H(source || source_id || updated_at || body)`.
    pub dedup_hash: String,
}

/// A chunk of a document's body text, stored in the `chunks` table.
///
/// Documents are split into chunks by the [`crate::chunk`] module to enable
/// granular retrieval and embedding. Each chunk has:
///
/// - A deterministic UUID (derived from document_id + chunk_index)
/// - A contiguous `chunk_index` starting at 0
/// - A SHA-256 `hash` of its text content, used by the embedding pipeline
///   to detect when re-embedding is needed (staleness detection)
#[derive(Debug, Clone)]
pub struct Chunk {
    /// UUID v4 primary key.
    pub id: String,
    /// Foreign key to the parent [`Document`].
    pub document_id: String,
    /// Zero-based index within the document's chunk sequence.
    pub chunk_index: i64,
    /// Chunk text content.
    pub text: String,
    /// SHA-256 hash of `text`, used for embedding staleness detection.
    pub hash: String,
}

/// A search result returned from the query engine.
///
/// Contains the document metadata, a relevance `score` normalized to
/// `[0.0, 1.0]`, and a `snippet` extracted from the best-matching chunk.
///
/// Used internally by the CLI; the HTTP server uses [`crate::search::SearchResultItem`]
/// which has the same shape but derives `Serialize`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    /// Document UUID.
    pub id: String,
    /// Document title.
    pub title: Option<String>,
    /// Connector name.
    pub source: String,
    /// Source identifier.
    pub source_id: String,
    /// Last modification timestamp (Unix epoch seconds).
    pub updated_at: i64,
    /// Relevance score in `[0.0, 1.0]`.
    pub score: f64,
    /// Text excerpt from the best-matching chunk.
    pub snippet: String,
    /// Web-browsable URL, if available.
    pub source_url: Option<String>,
}
