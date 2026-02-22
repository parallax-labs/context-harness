//! Database schema migrations.
//!
//! Creates all required tables and ensures idempotent execution.
//! Designed to be run via `ctx init`.
//!
//! # Schema
//!
//! ```text
//! ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
//! │  documents   │──┐  │   chunks     │──┐  │  embeddings  │
//! │              │  │  │              │  │  │              │
//! │ id (PK)      │  │  │ id (PK)      │  │  │ chunk_id(PK) │
//! │ source       │  └──│ document_id  │  └──│ model        │
//! │ source_id    │     │ chunk_index  │     │ dims         │
//! │ source_url   │     │ text         │     │ created_at   │
//! │ title        │     │ hash         │     │ hash         │
//! │ author       │     └──────────────┘     └──────────────┘
//! │ created_at   │
//! │ updated_at   │     ┌──────────────┐     ┌──────────────┐
//! │ content_type │     │  chunks_fts  │     │chunk_vectors │
//! │ body         │     │  (FTS5)      │     │              │
//! │ metadata_json│     │ chunk_id     │     │ chunk_id(PK) │
//! │ raw_json     │     │ document_id  │     │ document_id  │
//! │ dedup_hash   │     │ text         │     │ embedding    │
//! └──────────────┘     └──────────────┘     └──────────────┘
//!
//! ┌──────────────┐
//! │ checkpoints  │
//! │              │
//! │ source (PK)  │
//! │ cursor       │
//! │ updated_at   │
//! └──────────────┘
//! ```
//!
//! # Tables
//!
//! | Table | Purpose |
//! |-------|---------|
//! | `documents` | Normalized document metadata and body text |
//! | `chunks` | Text segments with content hashes |
//! | `checkpoints` | Incremental sync cursors per connector |
//! | `chunks_fts` | FTS5 full-text index over chunk text (BM25) |
//! | `embeddings` | Embedding metadata (model, dims, hash) |
//! | `chunk_vectors` | Embedding vectors stored as BLOBs |
//!
//! # Indexes
//!
//! - `idx_chunks_document_id` — fast chunk lookup by document
//! - `idx_documents_source` — fast document filtering by connector
//! - `idx_documents_updated_at` — efficient date range queries
//! - `idx_chunk_vectors_document_id` — fast vector lookup by document
//!
//! # Idempotency
//!
//! All operations use `CREATE TABLE IF NOT EXISTS` or check for existing
//! objects before creation. Running `ctx init` multiple times is safe.

use anyhow::Result;

use crate::config::Config;
use crate::db;

/// Run all database migrations.
///
/// Creates all tables, indexes, and virtual tables required by Context
/// Harness. Safe to call multiple times — all operations are idempotent.
///
/// # Tables Created
///
/// - `documents` — normalized document storage
/// - `chunks` — text segments with content hashes
/// - `checkpoints` — incremental sync cursors
/// - `chunks_fts` — FTS5 full-text search index
/// - `embeddings` — embedding metadata (model, dims, staleness hash)
/// - `chunk_vectors` — embedding vector BLOBs
///
/// # Errors
///
/// Returns an error if the database connection fails or any SQL statement
/// cannot be executed.
pub async fn run_migrations(config: &Config) -> Result<()> {
    let pool = db::connect(config).await?;

    // Create documents table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS documents (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_url TEXT,
            title TEXT,
            author TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            content_type TEXT NOT NULL DEFAULT 'text/plain',
            body TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            raw_json TEXT,
            dedup_hash TEXT NOT NULL,
            UNIQUE(source, source_id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Create chunks table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chunks (
            id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL,
            chunk_index INTEGER NOT NULL,
            text TEXT NOT NULL,
            hash TEXT NOT NULL,
            UNIQUE(document_id, chunk_index),
            FOREIGN KEY (document_id) REFERENCES documents(id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Create checkpoints table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS checkpoints (
            source TEXT PRIMARY KEY,
            cursor TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Create FTS5 virtual table over chunks (not idempotent natively, check first)
    let fts_exists: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
    )
    .fetch_one(&pool)
    .await?;

    if !fts_exists {
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE chunks_fts USING fts5(
                chunk_id UNINDEXED,
                document_id UNINDEXED,
                text
            )
            "#,
        )
        .execute(&pool)
        .await?;
    }

    // Embeddings metadata table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS embeddings (
            chunk_id TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            dims INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            hash TEXT NOT NULL,
            FOREIGN KEY (chunk_id) REFERENCES chunks(id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Chunk vectors table (stores embedding BLOBs)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chunk_vectors (
            chunk_id TEXT PRIMARY KEY,
            document_id TEXT NOT NULL,
            embedding BLOB NOT NULL,
            FOREIGN KEY (chunk_id) REFERENCES chunks(id),
            FOREIGN KEY (document_id) REFERENCES documents(id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    // Create indexes for common query patterns
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chunks_document_id ON chunks(document_id)")
        .execute(&pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_documents_source ON documents(source)")
        .execute(&pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_documents_updated_at ON documents(updated_at DESC)",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chunk_vectors_document_id ON chunk_vectors(document_id)",
    )
    .execute(&pool)
    .await?;

    pool.close().await;
    Ok(())
}
