//! SQLite-backed [`Store`] implementation.
//!
//! Maps each [`Store`] operation to the existing SQLite queries used by
//! the ingestion, search, and retrieval modules.

use anyhow::Result;
use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use context_harness_core::embedding::{blob_to_vec, cosine_similarity, vec_to_blob};
use context_harness_core::models::{Chunk, Document};
use context_harness_core::store::{
    ChunkCandidate, ChunkResponse, DocumentMetadata, DocumentResponse, Store,
};

/// SQLite implementation of the [`Store`] trait.
///
/// Wraps a [`SqlitePool`] and translates every `Store` method into one
/// or more SQL statements against the existing schema (documents, chunks,
/// chunks_fts, chunk_vectors, embeddings).
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    #[allow(dead_code)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}

#[async_trait]
impl Store for SqliteStore {
    async fn upsert_document(&self, doc: &Document) -> Result<String> {
        sqlx::query(
            r#"
            INSERT INTO documents (id, source, source_id, source_url, title, author,
                                   created_at, updated_at, content_type, body,
                                   metadata_json, raw_json, dedup_hash)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source, source_id) DO UPDATE SET
                source_url = excluded.source_url,
                title = excluded.title,
                author = excluded.author,
                updated_at = excluded.updated_at,
                content_type = excluded.content_type,
                body = excluded.body,
                metadata_json = excluded.metadata_json,
                raw_json = excluded.raw_json,
                dedup_hash = excluded.dedup_hash
            "#,
        )
        .bind(&doc.id)
        .bind(&doc.source)
        .bind(&doc.source_id)
        .bind(&doc.source_url)
        .bind(&doc.title)
        .bind(&doc.author)
        .bind(doc.created_at)
        .bind(doc.updated_at)
        .bind(&doc.content_type)
        .bind(&doc.body)
        .bind(&doc.metadata_json)
        .bind(&doc.raw_json)
        .bind(&doc.dedup_hash)
        .execute(&self.pool)
        .await?;

        Ok(doc.id.clone())
    }

    async fn replace_chunks(
        &self,
        doc_id: &str,
        chunks: &[Chunk],
        vectors: Option<&[Vec<f32>]>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM chunk_vectors WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
        )
        .bind(doc_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
        )
        .bind(doc_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM chunks_fts WHERE document_id = ?")
            .bind(doc_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM chunks WHERE document_id = ?")
            .bind(doc_id)
            .execute(&mut *tx)
            .await?;

        for (i, chunk) in chunks.iter().enumerate() {
            sqlx::query(
                "INSERT INTO chunks (id, document_id, chunk_index, text, hash) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&chunk.id)
            .bind(&chunk.document_id)
            .bind(chunk.chunk_index)
            .bind(&chunk.text)
            .bind(&chunk.hash)
            .execute(&mut *tx)
            .await?;

            sqlx::query("INSERT INTO chunks_fts (chunk_id, document_id, text) VALUES (?, ?, ?)")
                .bind(&chunk.id)
                .bind(&chunk.document_id)
                .bind(&chunk.text)
                .execute(&mut *tx)
                .await?;

            if let Some(vecs) = vectors {
                if let Some(vec) = vecs.get(i) {
                    let blob = vec_to_blob(vec);
                    sqlx::query(
                        r#"
                        INSERT INTO chunk_vectors (chunk_id, document_id, embedding)
                        VALUES (?, ?, ?)
                        ON CONFLICT(chunk_id) DO UPDATE SET
                            document_id = excluded.document_id,
                            embedding = excluded.embedding
                        "#,
                    )
                    .bind(&chunk.id)
                    .bind(doc_id)
                    .bind(&blob)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }

        tx.commit().await?;
        Ok(())
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
        let now = chrono::Utc::now().timestamp();
        let blob = vec_to_blob(vector);

        sqlx::query(
            r#"
            INSERT INTO embeddings (chunk_id, model, dims, created_at, hash)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(chunk_id) DO UPDATE SET
                model = excluded.model,
                dims = excluded.dims,
                created_at = excluded.created_at,
                hash = excluded.hash
            "#,
        )
        .bind(chunk_id)
        .bind(model)
        .bind(dims as i64)
        .bind(now)
        .bind(content_hash)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO chunk_vectors (chunk_id, document_id, embedding)
            VALUES (?, ?, ?)
            ON CONFLICT(chunk_id) DO UPDATE SET
                document_id = excluded.document_id,
                embedding = excluded.embedding
            "#,
        )
        .bind(chunk_id)
        .bind(doc_id)
        .bind(&blob)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_document(&self, id: &str) -> Result<Option<DocumentResponse>> {
        let doc_row = sqlx::query(
            "SELECT id, source, source_id, source_url, title, author, created_at, updated_at, content_type, body, metadata_json FROM documents WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let doc_row = match doc_row {
            Some(row) => row,
            None => return Ok(None),
        };

        let created_at: i64 = doc_row.get("created_at");
        let updated_at: i64 = doc_row.get("updated_at");
        let metadata_json: String = doc_row.get("metadata_json");

        let metadata: serde_json::Value =
            serde_json::from_str(&metadata_json).unwrap_or(serde_json::json!({}));

        let chunk_rows = sqlx::query(
            "SELECT chunk_index, text FROM chunks WHERE document_id = ? ORDER BY chunk_index ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let chunks: Vec<ChunkResponse> = chunk_rows
            .iter()
            .map(|row| ChunkResponse {
                index: row.get("chunk_index"),
                text: row.get("text"),
            })
            .collect();

        Ok(Some(DocumentResponse {
            id: doc_row.get("id"),
            source: doc_row.get("source"),
            source_id: doc_row.get("source_id"),
            source_url: doc_row.get("source_url"),
            title: doc_row.get("title"),
            author: doc_row.get("author"),
            created_at: format_ts_iso(created_at),
            updated_at: format_ts_iso(updated_at),
            content_type: doc_row.get("content_type"),
            body: doc_row.get("body"),
            metadata,
            chunks,
        }))
    }

    async fn get_document_metadata(&self, id: &str) -> Result<Option<DocumentMetadata>> {
        let row = sqlx::query(
            "SELECT id, title, source, source_id, updated_at, source_url FROM documents WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| DocumentMetadata {
            id: r.get("id"),
            title: r.get("title"),
            source: r.get("source"),
            source_id: r.get("source_id"),
            source_url: r.get("source_url"),
            updated_at: r.get("updated_at"),
        }))
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
        _source: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        let rows = sqlx::query(
            r#"
            SELECT chunk_id, document_id, rank,
                   snippet(chunks_fts, 2, '>>>', '<<<', '...', 48) AS snippet
            FROM chunks_fts
            WHERE chunks_fts MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let candidates: Vec<ChunkCandidate> = rows
            .iter()
            .map(|row| {
                let rank: f64 = row.get("rank");
                ChunkCandidate {
                    chunk_id: row.get("chunk_id"),
                    document_id: row.get("document_id"),
                    raw_score: -rank,
                    snippet: row.get("snippet"),
                }
            })
            .collect();

        Ok(candidates)
    }

    async fn vector_search(
        &self,
        query_vec: &[f32],
        limit: i64,
        _source: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        let rows = sqlx::query(
            r#"
            SELECT cv.chunk_id, cv.document_id, cv.embedding,
                   COALESCE(substr(c.text, 1, 240), '') AS snippet
            FROM chunk_vectors cv
            JOIN chunks c ON c.id = cv.chunk_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut candidates: Vec<ChunkCandidate> = rows
            .iter()
            .map(|row| {
                let blob: Vec<u8> = row.get("embedding");
                let vec = blob_to_vec(&blob);
                let similarity = cosine_similarity(query_vec, &vec) as f64;
                ChunkCandidate {
                    chunk_id: row.get("chunk_id"),
                    document_id: row.get("document_id"),
                    raw_score: similarity,
                    snippet: row.get("snippet"),
                }
            })
            .collect();

        candidates.sort_by(|a, b| {
            b.raw_score
                .partial_cmp(&a.raw_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limit as usize);

        Ok(candidates)
    }
}
