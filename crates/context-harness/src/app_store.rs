//! Application-level storage boundary for native Context Harness operations.
//!
//! [`context_harness_core::store::Store`] covers reusable search and retrieval
//! behavior. [`AppStore`] adds CLI/native responsibilities that are still
//! canonical SQLite operations today: migrations, sync checkpoints, source item
//! writes, embedding maintenance, stats, and export views.

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use context_harness_core::models::{Chunk, Document};
use context_harness_core::store::{ChunkCandidate, DocumentMetadata, DocumentResponse, Store};

use crate::config::Config;
use crate::db;
use crate::migrate;
use crate::models::SourceItem;
use crate::sqlite_store::SqliteStore;

/// A chunk that needs embedding because its embedding is missing or stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChunk {
    pub chunk_id: String,
    pub document_id: String,
    pub text: String,
    pub text_hash: String,
}

/// Per-source database statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStats {
    pub source: String,
    pub doc_count: i64,
    pub chunk_count: i64,
    pub embedded_count: i64,
    pub last_sync_ts: Option<i64>,
}

/// Database statistics used by `ctx stats`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreStats {
    pub total_docs: i64,
    pub total_chunks: i64,
    pub total_embedded: i64,
    pub db_size_bytes: u64,
    pub sources: Vec<SourceStats>,
}

/// Export payload used by `ctx export`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportData {
    pub documents: Vec<ExportDocument>,
    pub chunks: Vec<ExportChunk>,
}

/// Exported document row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportDocument {
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub updated_at: i64,
    pub body: String,
}

/// Exported chunk row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportChunk {
    pub id: String,
    pub document_id: String,
    pub chunk_index: i64,
    pub text: String,
}

/// App-level storage operations layered on top of core search storage.
#[async_trait]
pub trait AppStore: Store {
    #[allow(dead_code)]
    async fn initialize(&self) -> Result<()>;
    async fn get_checkpoint(&self, source: &str) -> Result<Option<i64>>;
    async fn set_checkpoint(&self, source: &str, cursor: i64) -> Result<()>;
    async fn upsert_source_item(&self, item: &SourceItem) -> Result<String>;
    async fn find_pending_chunks(
        &self,
        model: &str,
        limit: Option<usize>,
    ) -> Result<Vec<PendingChunk>>;
    async fn get_embedding_hash(&self, chunk_id: &str, model: &str) -> Result<Option<String>>;
    async fn clear_embeddings(&self) -> Result<()>;
    async fn stats(&self) -> Result<StoreStats>;
    async fn export_index(&self) -> Result<ExportData>;
}

/// SQLite-backed [`AppStore`] implementation.
pub struct SqliteAppStore {
    config: Config,
    pool: SqlitePool,
}

impl SqliteAppStore {
    pub async fn connect(config: &Config) -> Result<Self> {
        let pool = db::connect(config).await?;
        Ok(Self {
            config: config.clone(),
            pool,
        })
    }

    pub async fn initialize_config(config: &Config) -> Result<()> {
        migrate::run_migrations(config).await
    }

    #[allow(dead_code)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    fn core_store(&self) -> SqliteStore {
        SqliteStore::new(self.pool.clone())
    }
}

#[async_trait]
impl Store for SqliteAppStore {
    async fn upsert_document(&self, doc: &Document) -> Result<String> {
        self.core_store().upsert_document(doc).await
    }

    async fn replace_chunks(
        &self,
        doc_id: &str,
        chunks: &[Chunk],
        vectors: Option<&[Vec<f32>]>,
    ) -> Result<()> {
        self.core_store()
            .replace_chunks(doc_id, chunks, vectors)
            .await
    }

    async fn upsert_embedding(
        &self,
        chunk_id: &str,
        doc_id: &str,
        vector: &[f32],
        model: &str,
        dims: usize,
        content_hash: &str,
    ) -> Result<()> {
        self.core_store()
            .upsert_embedding(chunk_id, doc_id, vector, model, dims, content_hash)
            .await
    }

    async fn get_document(&self, id: &str) -> Result<Option<DocumentResponse>> {
        self.core_store().get_document(id).await
    }

    async fn get_document_metadata(&self, id: &str) -> Result<Option<DocumentMetadata>> {
        self.core_store().get_document_metadata(id).await
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
        source: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        self.core_store()
            .keyword_search(query, limit, source, since)
            .await
    }

    async fn vector_search(
        &self,
        query_vec: &[f32],
        limit: i64,
        source: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        self.core_store()
            .vector_search(query_vec, limit, source, since)
            .await
    }
}

#[async_trait]
impl AppStore for SqliteAppStore {
    async fn initialize(&self) -> Result<()> {
        migrate::run_migrations(&self.config).await
    }

    async fn get_checkpoint(&self, source: &str) -> Result<Option<i64>> {
        let result: Option<String> =
            sqlx::query_scalar("SELECT cursor FROM checkpoints WHERE source = ?")
                .bind(source)
                .fetch_optional(&self.pool)
                .await?;

        Ok(result.and_then(|s| s.parse::<i64>().ok()))
    }

    async fn set_checkpoint(&self, source: &str, cursor: i64) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO checkpoints (source, cursor, updated_at) VALUES (?, ?, ?)
            ON CONFLICT(source) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at
            "#,
        )
        .bind(source)
        .bind(cursor.to_string())
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn upsert_source_item(&self, item: &SourceItem) -> Result<String> {
        let doc = source_item_to_document(&self.pool, item).await?;
        self.upsert_document(&doc).await
    }

    async fn find_pending_chunks(
        &self,
        model: &str,
        limit: Option<usize>,
    ) -> Result<Vec<PendingChunk>> {
        let limit_val = limit.unwrap_or(usize::MAX) as i64;

        let rows = sqlx::query(
            r#"
            SELECT c.id AS chunk_id, c.document_id, c.text, c.hash AS chunk_hash
            FROM chunks c
            LEFT JOIN embeddings e ON e.chunk_id = c.id AND e.model = ?
            WHERE e.chunk_id IS NULL OR e.hash != c.hash
            ORDER BY c.document_id, c.chunk_index
            LIMIT ?
            "#,
        )
        .bind(model)
        .bind(limit_val)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|row| {
                let text: String = row.get("text");
                PendingChunk {
                    chunk_id: row.get("chunk_id"),
                    document_id: row.get("document_id"),
                    text_hash: row.get("chunk_hash"),
                    text,
                }
            })
            .collect())
    }

    async fn get_embedding_hash(&self, chunk_id: &str, model: &str) -> Result<Option<String>> {
        let hash =
            sqlx::query_scalar("SELECT hash FROM embeddings WHERE chunk_id = ? AND model = ?")
                .bind(chunk_id)
                .bind(model)
                .fetch_optional(&self.pool)
                .await?;
        Ok(hash)
    }

    async fn clear_embeddings(&self) -> Result<()> {
        sqlx::query("DELETE FROM chunk_vectors")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM embeddings")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn stats(&self) -> Result<StoreStats> {
        let total_docs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
            .fetch_one(&self.pool)
            .await?;
        let total_chunks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks")
            .fetch_one(&self.pool)
            .await?;
        let total_embedded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_vectors")
            .fetch_one(&self.pool)
            .await?;
        let db_size_bytes = std::fs::metadata(&self.config.db.path)
            .map(|m| m.len())
            .unwrap_or(0);

        let source_rows = sqlx::query(
            r#"
            SELECT
                d.source,
                COUNT(DISTINCT d.id) AS doc_count,
                COUNT(DISTINCT c.id) AS chunk_count,
                COUNT(DISTINCT cv.chunk_id) AS embedded_count
            FROM documents d
            LEFT JOIN chunks c ON c.document_id = d.id
            LEFT JOIN chunk_vectors cv ON cv.chunk_id = c.id
            GROUP BY d.source
            ORDER BY doc_count DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let checkpoint_rows = sqlx::query("SELECT source, updated_at FROM checkpoints")
            .fetch_all(&self.pool)
            .await?;

        let mut sources = Vec::new();
        for row in &source_rows {
            let source: String = row.get("source");
            let last_sync_ts = checkpoint_rows
                .iter()
                .find(|cp| {
                    let cp_source: String = cp.get("source");
                    cp_source == source
                })
                .map(|cp| cp.get::<i64, _>("updated_at"));

            sources.push(SourceStats {
                source,
                doc_count: row.get("doc_count"),
                chunk_count: row.get("chunk_count"),
                embedded_count: row.get("embedded_count"),
                last_sync_ts,
            });
        }

        Ok(StoreStats {
            total_docs,
            total_chunks,
            total_embedded,
            db_size_bytes,
            sources,
        })
    }

    async fn export_index(&self) -> Result<ExportData> {
        let doc_rows = sqlx::query(
            "SELECT id, source, source_id, source_url, title, updated_at, body \
             FROM documents ORDER BY source_id",
        )
        .fetch_all(&self.pool)
        .await?;

        let chunk_rows = sqlx::query(
            "SELECT id, document_id, chunk_index, text \
             FROM chunks ORDER BY document_id, chunk_index",
        )
        .fetch_all(&self.pool)
        .await?;

        let documents = doc_rows
            .iter()
            .map(|row| ExportDocument {
                id: row.get("id"),
                source: row.get("source"),
                source_id: row.get("source_id"),
                source_url: row.get("source_url"),
                title: row.get("title"),
                updated_at: row.get("updated_at"),
                body: row.get("body"),
            })
            .collect();

        let chunks = chunk_rows
            .iter()
            .map(|row| ExportChunk {
                id: row.get("id"),
                document_id: row.get("document_id"),
                chunk_index: row.get("chunk_index"),
                text: row.get("text"),
            })
            .collect();

        Ok(ExportData { documents, chunks })
    }
}

async fn source_item_to_document(pool: &SqlitePool, item: &SourceItem) -> Result<Document> {
    let dedup_hash = dedup_hash(item);
    let existing_id: Option<String> =
        sqlx::query_scalar("SELECT id FROM documents WHERE source = ? AND source_id = ?")
            .bind(&item.source)
            .bind(&item.source_id)
            .fetch_optional(pool)
            .await?;

    Ok(Document {
        id: existing_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        source: item.source.clone(),
        source_id: item.source_id.clone(),
        source_url: item.source_url.clone(),
        title: item.title.clone(),
        author: item.author.clone(),
        created_at: item.created_at.timestamp(),
        updated_at: item.updated_at.timestamp(),
        content_type: item.content_type.clone(),
        body: item.body.clone(),
        metadata_json: item.metadata_json.clone(),
        raw_json: item.raw_json.clone(),
        dedup_hash,
    })
}

fn dedup_hash(item: &SourceItem) -> String {
    let mut hasher = Sha256::new();
    hasher.update(item.source.as_bytes());
    hasher.update(item.source_id.as_bytes());
    hasher.update(item.updated_at.timestamp().to_le_bytes());
    hasher.update(item.body.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}
