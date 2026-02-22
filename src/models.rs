//! Core data models used throughout Context Harness.
//!
//! These types represent the documents, chunks, and search results that flow
//! through the ingestion and retrieval pipeline.

use chrono::{DateTime, Utc};

/// Raw item produced by a connector before normalization.
#[derive(Debug, Clone)]
pub struct SourceItem {
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub content_type: String,
    pub body: String,
    pub metadata_json: String,
    pub raw_json: Option<String>,
}

/// Normalized document stored in SQLite.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Document {
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub content_type: String,
    pub body: String,
    pub metadata_json: String,
    pub raw_json: Option<String>,
    pub dedup_hash: String,
}

/// A chunk of a document's body text.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub chunk_index: i64,
    pub text: String,
    pub hash: String,
}

/// A search result returned from the query engine.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    pub id: String,
    pub title: Option<String>,
    pub source: String,
    pub source_id: String,
    pub updated_at: i64,
    pub score: f64,
    pub snippet: String,
    pub source_url: Option<String>,
}
