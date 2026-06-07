//! Optional vector-index boundary.
//!
//! SQLite remains the canonical store for documents, chunks, FTS5 rows, and
//! embedding metadata. A [`VectorIndex`] only retrieves vector candidates for
//! semantic search; core hybrid scoring still consumes [`ChunkCandidate`]s.

use anyhow::Result;
use async_trait::async_trait;

use context_harness_core::store::ChunkCandidate;
use context_harness_core::store::Store;

use crate::sqlite_store::SqliteStore;

/// A vector row available to an optional vector index.
#[derive(Debug, Clone)]
pub struct VectorRecord {
    pub chunk_id: String,
    pub document_id: String,
    pub vector: Vec<f32>,
    pub model: String,
    pub dims: usize,
    pub content_hash: String,
}

/// Search controls passed to vector-index backends.
#[derive(Debug, Clone, Copy, Default)]
pub struct VectorSearchOptions<'a> {
    pub limit: i64,
    pub source: Option<&'a str>,
    pub since: Option<&'a str>,
}

/// Health information for the configured vector-index backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexHealth {
    pub enabled: bool,
    pub available: bool,
    pub backend: String,
    pub message: Option<String>,
}

/// Optional semantic-search accelerator.
#[async_trait]
pub trait VectorIndex: Send + Sync {
    async fn upsert(&self, record: &VectorRecord) -> Result<()>;
    async fn delete_chunk(&self, chunk_id: &str) -> Result<()>;
    async fn delete_document(&self, document_id: &str) -> Result<()>;
    async fn search(
        &self,
        query_vec: &[f32],
        options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>>;
    async fn health(&self) -> Result<VectorIndexHealth>;
}

/// Disabled vector index. Callers should use the configured fallback.
pub struct DisabledVectorIndex;

#[async_trait]
impl VectorIndex for DisabledVectorIndex {
    async fn upsert(&self, _record: &VectorRecord) -> Result<()> {
        Ok(())
    }

    async fn delete_chunk(&self, _chunk_id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete_document(&self, _document_id: &str) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        _query_vec: &[f32],
        _options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>> {
        Ok(Vec::new())
    }

    async fn health(&self) -> Result<VectorIndexHealth> {
        Ok(VectorIndexHealth {
            enabled: false,
            available: false,
            backend: "disabled".to_string(),
            message: Some("vector index disabled; SQLite fallback remains canonical".to_string()),
        })
    }
}

/// Exact brute-force SQLite vector scan used as the behavioral baseline.
pub struct BruteForceSqliteVectorIndex {
    store: SqliteStore,
}

impl BruteForceSqliteVectorIndex {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl VectorIndex for BruteForceSqliteVectorIndex {
    async fn upsert(&self, _record: &VectorRecord) -> Result<()> {
        Ok(())
    }

    async fn delete_chunk(&self, _chunk_id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete_document(&self, _document_id: &str) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        query_vec: &[f32],
        options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>> {
        self.store
            .vector_search(query_vec, options.limit, options.source, options.since)
            .await
    }

    async fn health(&self) -> Result<VectorIndexHealth> {
        Ok(VectorIndexHealth {
            enabled: true,
            available: true,
            backend: "sqlite-bruteforce".to_string(),
            message: Some("exact SQLite vector scan".to_string()),
        })
    }
}
