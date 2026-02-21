use anyhow::Result;

use crate::config::Config;
use crate::db;

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

    // Create FTS5 virtual table over chunks
    // FTS5 CREATE is not idempotent natively, so we check first
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

    // Create indexes
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

    pool.close().await;
    Ok(())
}
