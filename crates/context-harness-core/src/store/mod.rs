//! Storage abstraction for Context Harness.
//!
//! The [`Store`] trait defines all storage operations needed by the core
//! search and retrieval pipeline, enabling pluggable backends (SQLite,
//! in-memory, future WASM-compatible stores).
//!
//! Implementations must be `Send + Sync` to work with async runtimes.

pub mod memory;

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;

use crate::models::{Chunk, Document};

/// A candidate chunk returned from keyword or vector search.
///
/// Carries enough information to perform score normalization, hybrid
/// merging, and document aggregation without additional DB round-trips.
#[derive(Debug, Clone)]
pub struct ChunkCandidate {
    /// Chunk UUID.
    pub chunk_id: String,
    /// Parent document UUID.
    pub document_id: String,
    /// Raw score from the search backend (BM25 rank or cosine similarity).
    pub raw_score: f64,
    /// Text excerpt for display.
    pub snippet: String,
}

/// Full document response including metadata, body, and chunks.
///
/// Matches the `context.get` response shape defined in `docs/SCHEMAS.md`.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub content_type: String,
    pub body: String,
    pub metadata: serde_json::Value,
    pub chunks: Vec<ChunkResponse>,
}

/// A single chunk within a [`DocumentResponse`].
#[derive(Debug, Clone, Serialize)]
pub struct ChunkResponse {
    pub index: i64,
    pub text: String,
}

/// Lightweight document metadata for search result enrichment.
///
/// Contains only the fields needed to build a [`SearchResultItem`](crate::search::SearchResultItem),
/// avoiding the cost of fetching the full document body.
#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    pub id: String,
    pub title: Option<String>,
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub updated_at: i64,
}

/// Abstract storage backend for Context Harness.
///
/// All operations are async (via `async-trait`) to support both native
/// runtimes (tokio) and future WASM environments. In-memory
/// implementations return immediately-ready futures.
///
/// # Operations
///
/// | Method | Purpose |
/// |--------|---------|
/// | [`upsert_document`](Store::upsert_document) | Insert or update a document |
/// | [`replace_chunks`](Store::replace_chunks) | Replace all chunks for a document |
/// | [`upsert_embedding`](Store::upsert_embedding) | Store an embedding vector for a chunk |
/// | [`get_document`](Store::get_document) | Retrieve full document with chunks |
/// | [`get_document_metadata`](Store::get_document_metadata) | Retrieve lightweight doc metadata |
/// | [`keyword_search`](Store::keyword_search) | Full-text keyword search |
/// | [`vector_search`](Store::vector_search) | Cosine similarity vector search |
#[async_trait]
pub trait Store: Send + Sync {
    /// Insert or update a document.
    ///
    /// Returns the document ID (existing or newly generated).
    async fn upsert_document(&self, doc: &Document) -> Result<String>;

    /// Replace all chunks for a document, optionally storing vectors.
    async fn replace_chunks(
        &self,
        doc_id: &str,
        chunks: &[Chunk],
        vectors: Option<&[Vec<f32>]>,
    ) -> Result<()>;

    /// Store or update an embedding vector for a chunk.
    async fn upsert_embedding(
        &self,
        chunk_id: &str,
        doc_id: &str,
        vector: &[f32],
        model: &str,
        dims: usize,
        content_hash: &str,
    ) -> Result<()>;

    /// Retrieve a full document with all its chunks, by ID.
    async fn get_document(&self, id: &str) -> Result<Option<DocumentResponse>>;

    /// Retrieve lightweight metadata for a document, by ID.
    async fn get_document_metadata(&self, id: &str) -> Result<Option<DocumentMetadata>>;

    /// Perform keyword (full-text) search, returning candidate chunks.
    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
        source: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>>;

    /// Perform vector similarity search, returning candidate chunks.
    async fn vector_search(
        &self,
        query_vec: &[f32],
        limit: i64,
        source: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>>;
}
