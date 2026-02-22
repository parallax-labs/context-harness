//! Ingestion pipeline orchestration.
//!
//! Coordinates the full sync flow: connector → normalization → chunking →
//! embedding → storage. Supports incremental sync via checkpoints and
//! inline embedding (non-fatal on failure).
//!
//! # Sync Pipeline
//!
//! The `run_sync` function implements the following pipeline:
//!
//! 1. **Load checkpoint** — determines the last successful sync timestamp
//!    for the specified connector (skipped with `--full`).
//! 2. **Scan connector** — dispatches to the appropriate connector
//!    (`filesystem`, `git`, or `s3`) to fetch [`SourceItem`]s.
//! 3. **Filter** — applies checkpoint, `--since`, `--until`, and `--limit`
//!    filters to the collected items.
//! 4. **Upsert documents** — inserts or updates each item in the `documents`
//!    table, computing a SHA-256 deduplication hash.
//! 5. **Replace chunks** — deletes old chunks (and their embeddings/FTS entries)
//!    for the document, then inserts fresh chunks.
//! 6. **Inline embed** — if embeddings are enabled, embeds new chunks
//!    immediately (non-fatal: failures are logged but do not abort the sync).
//! 7. **Update checkpoint** — persists the latest `updated_at` timestamp
//!    so the next incremental sync can skip unchanged items.
//!
//! # Deduplication
//!
//! Each document is identified by `(source, source_id)`. If a document with
//! the same composite key already exists, it is updated via an `ON CONFLICT`
//! upsert. The `dedup_hash` field is a SHA-256 digest of source + source_id +
//! updated_at + body, enabling downstream consumers to detect changes.
//!
//! # Checkpointing
//!
//! Checkpoints are stored in the `checkpoints` table as `(source, cursor)`
//! pairs. The cursor is the maximum `updated_at` timestamp seen during the
//! sync. On subsequent runs, only items newer than the checkpoint are processed.

use anyhow::{bail, Result};
use chrono::NaiveDate;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::chunk::chunk_text;
use crate::config::Config;
use crate::connector_fs;
use crate::connector_git;
use crate::connector_s3;
use crate::db;
use crate::embed_cmd;
use crate::models::SourceItem;

/// Runs the full ingestion pipeline for the specified connector.
///
/// This is the entry point for `ctx sync <connector>`. It orchestrates
/// scanning, filtering, document upsert, chunking, embedding, and
/// checkpoint updates.
///
/// # Arguments
///
/// - `config` — application configuration.
/// - `connector` — the connector name (`"filesystem"`, `"git"`, or `"s3"`).
/// - `full` — if `true`, ignores the existing checkpoint and reprocesses all items.
/// - `dry_run` — if `true`, scans and counts items without writing to the database.
/// - `since` — optional `YYYY-MM-DD` date; only items updated on or after this date are processed.
/// - `until` — optional `YYYY-MM-DD` date; only items updated on or before this date are processed.
/// - `limit` — optional maximum number of items to process.
///
/// # Returns
///
/// A `Result` indicating success or an `anyhow::Error` describing the failure.
///
/// # Errors
///
/// Returns an error if:
/// - The specified connector is unknown.
/// - The connector fails to scan (e.g., missing configuration, network error).
/// - A database operation fails.
pub async fn run_sync(
    config: &Config,
    connector: &str,
    full: bool,
    dry_run: bool,
    since: Option<String>,
    until: Option<String>,
    limit: Option<usize>,
) -> Result<()> {
    let pool = db::connect(config).await?;

    // Load checkpoint
    let checkpoint: Option<i64> = if full {
        None
    } else {
        get_checkpoint(&pool, connector).await?
    };

    // Scan the appropriate connector
    let mut items = match connector {
        "filesystem" => connector_fs::scan_filesystem(config)?,
        "git" => connector_git::scan_git(config)?,
        "s3" => connector_s3::scan_s3(config).await?,
        _ => bail!(
            "Unknown connector: '{}'. Available: filesystem, git, s3",
            connector
        ),
    };

    // Filter by checkpoint (skip files not modified since checkpoint)
    if let Some(cp) = checkpoint {
        items.retain(|item| item.updated_at.timestamp() > cp);
    }

    // Apply --since filter
    if let Some(ref since_str) = since {
        let since_date = NaiveDate::parse_from_str(since_str, "%Y-%m-%d")?;
        let since_ts = since_date
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        items.retain(|item| item.updated_at.timestamp() >= since_ts);
    }

    // Apply --until filter
    if let Some(ref until_str) = until {
        let until_date = NaiveDate::parse_from_str(until_str, "%Y-%m-%d")?;
        let until_ts = until_date
            .and_hms_opt(23, 59, 59)
            .unwrap()
            .and_utc()
            .timestamp();
        items.retain(|item| item.updated_at.timestamp() <= until_ts);
    }

    // Apply --limit
    if let Some(lim) = limit {
        items.truncate(lim);
    }

    if dry_run {
        println!("sync {} (dry-run)", connector);
        println!("  items found: {}", items.len());
        let total_chunks: usize = items
            .iter()
            .map(|item| chunk_text("tmp", &item.body, config.chunking.max_tokens).len())
            .sum();
        println!("  estimated chunks: {}", total_chunks);
        return Ok(());
    }

    let mut docs_upserted = 0u64;
    let mut chunks_written = 0u64;
    let mut embeddings_written = 0u64;
    let mut embeddings_pending = 0u64;
    let mut max_updated: i64 = checkpoint.unwrap_or(0);

    for item in &items {
        let doc_id = upsert_document(&pool, item).await?;
        let chunks = chunk_text(&doc_id, &item.body, config.chunking.max_tokens);
        let chunk_count = chunks.len() as u64;
        replace_chunks(&pool, &doc_id, &chunks).await?;

        // Inline embedding (non-fatal)
        let (emb_ok, emb_pending) = embed_cmd::embed_chunks_inline(config, &pool, &chunks).await;
        embeddings_written += emb_ok;
        embeddings_pending += emb_pending;

        docs_upserted += 1;
        chunks_written += chunk_count;

        let ts = item.updated_at.timestamp();
        if ts > max_updated {
            max_updated = ts;
        }
    }

    // Update checkpoint
    set_checkpoint(&pool, connector, max_updated).await?;

    println!("sync {}", connector);
    println!("  fetched: {} items", items.len());
    println!("  upserted documents: {}", docs_upserted);
    println!("  chunks written: {}", chunks_written);
    if config.embedding.is_enabled() {
        println!("  embeddings written: {}", embeddings_written);
        println!("  embeddings pending: {}", embeddings_pending);
    }
    println!("  checkpoint: {}", max_updated);
    println!("ok");

    pool.close().await;
    Ok(())
}

/// Inserts or updates a document in the `documents` table.
///
/// Computes a SHA-256 deduplication hash from the item's source, source_id,
/// updated_at timestamp, and body. If a document with the same `(source, source_id)`
/// already exists, it is updated with the new data; otherwise a new UUID is assigned.
///
/// # Returns
///
/// The document's UUID (existing or newly generated).
async fn upsert_document(pool: &SqlitePool, item: &SourceItem) -> Result<String> {
    // Compute dedup hash
    let mut hasher = Sha256::new();
    hasher.update(item.source.as_bytes());
    hasher.update(item.source_id.as_bytes());
    hasher.update(item.updated_at.timestamp().to_le_bytes());
    hasher.update(item.body.as_bytes());
    let dedup_hash = format!("{:x}", hasher.finalize());

    // Check if document exists
    let existing_id: Option<String> =
        sqlx::query_scalar("SELECT id FROM documents WHERE source = ? AND source_id = ?")
            .bind(&item.source)
            .bind(&item.source_id)
            .fetch_optional(pool)
            .await?;

    let doc_id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    sqlx::query(
        r#"
        INSERT INTO documents (id, source, source_id, source_url, title, author, created_at, updated_at, content_type, body, metadata_json, raw_json, dedup_hash)
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
    .bind(&doc_id)
    .bind(&item.source)
    .bind(&item.source_id)
    .bind(&item.source_url)
    .bind(&item.title)
    .bind(&item.author)
    .bind(item.created_at.timestamp())
    .bind(item.updated_at.timestamp())
    .bind(&item.content_type)
    .bind(&item.body)
    .bind(&item.metadata_json)
    .bind(&item.raw_json)
    .bind(&dedup_hash)
    .execute(pool)
    .await?;

    Ok(doc_id)
}

/// Atomically replaces all chunks (and their embeddings/FTS entries) for a document.
///
/// This function runs inside a single SQLite transaction to ensure consistency:
/// 1. Deletes old `chunk_vectors` rows for the document's chunks.
/// 2. Deletes old `embeddings` metadata rows for the document's chunks.
/// 3. Deletes old `chunks_fts` (FTS5) entries for the document.
/// 4. Deletes old `chunks` rows for the document.
/// 5. Inserts new chunks and their corresponding FTS entries.
///
/// # Arguments
///
/// - `pool` — the database connection pool.
/// - `document_id` — the UUID of the parent document.
/// - `chunks` — the new set of chunks to insert.
async fn replace_chunks(
    pool: &SqlitePool,
    document_id: &str,
    chunks: &[crate::models::Chunk],
) -> Result<()> {
    let mut tx = pool.begin().await?;

    // Delete old embeddings for this document's chunks
    sqlx::query(
        "DELETE FROM chunk_vectors WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
    )
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
    )
    .bind(document_id)
    .execute(&mut *tx)
    .await?;

    // Delete old FTS entries for this document's chunks
    sqlx::query("DELETE FROM chunks_fts WHERE document_id = ?")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;

    // Delete old chunks
    sqlx::query("DELETE FROM chunks WHERE document_id = ?")
        .bind(document_id)
        .execute(&mut *tx)
        .await?;

    // Insert new chunks + FTS entries
    for chunk in chunks {
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
    }

    tx.commit().await?;
    Ok(())
}

/// Retrieves the last sync checkpoint for a given connector.
///
/// Returns `Some(timestamp)` if a checkpoint exists, or `None` if this is
/// the first sync for the connector.
async fn get_checkpoint(pool: &SqlitePool, source: &str) -> Result<Option<i64>> {
    let result: Option<String> =
        sqlx::query_scalar("SELECT cursor FROM checkpoints WHERE source = ?")
            .bind(source)
            .fetch_optional(pool)
            .await?;

    Ok(result.and_then(|s| s.parse::<i64>().ok()))
}

/// Persists the sync checkpoint for a connector.
///
/// Uses an upsert to create or update the checkpoint row. The `cursor`
/// value is typically the maximum `updated_at` timestamp seen during the sync.
async fn set_checkpoint(pool: &SqlitePool, source: &str, cursor_val: i64) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO checkpoints (source, cursor, updated_at) VALUES (?, ?, ?)
        ON CONFLICT(source) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at
        "#,
    )
    .bind(source)
    .bind(cursor_val.to_string())
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}
