use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use crate::config::Config;
use crate::db;
use crate::embedding;

/// Find and embed chunks that are missing or have stale embeddings.
pub async fn run_embed_pending(
    config: &Config,
    limit: Option<usize>,
    batch_size_override: Option<usize>,
    dry_run: bool,
) -> Result<()> {
    if !config.embedding.is_enabled() {
        bail!("Embedding provider is disabled. Set [embedding] provider in config.");
    }

    let provider = embedding::create_provider(&config.embedding)?;
    let model_name = provider.model_name().to_string();
    let pool = db::connect(config).await?;
    let batch_size = batch_size_override.unwrap_or(config.embedding.batch_size);

    // Find chunks missing embeddings or with stale hashes
    let pending = find_pending_chunks(&pool, &model_name, limit).await?;

    if dry_run {
        println!("embed pending (dry-run)");
        println!("  chunks needing embeddings: {}", pending.len());
        return Ok(());
    }

    if pending.is_empty() {
        println!("embed pending");
        println!("  all chunks up to date");
        return Ok(());
    }

    let total = pending.len();
    let mut embedded = 0u64;
    let mut failed = 0u64;

    for batch in pending.chunks(batch_size) {
        let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();

        match embedding::embed_texts(provider.as_ref(), &config.embedding, &texts).await {
            Ok(vectors) => {
                for (item, vec) in batch.iter().zip(vectors.iter()) {
                    let blob = embedding::vec_to_blob(vec);
                    upsert_embedding(
                        &pool,
                        &item.chunk_id,
                        &item.document_id,
                        &model_name,
                        provider.dims(),
                        &item.text_hash,
                        &blob,
                    )
                    .await?;
                    embedded += 1;
                }
            }
            Err(e) => {
                eprintln!("Warning: embedding batch failed: {}", e);
                failed += batch.len() as u64;
            }
        }
    }

    println!("embed pending");
    println!("  total pending: {}", total);
    println!("  embedded: {}", embedded);
    println!("  failed: {}", failed);

    pool.close().await;
    Ok(())
}

/// Delete all embeddings and regenerate for all chunks.
pub async fn run_embed_rebuild(config: &Config, batch_size_override: Option<usize>) -> Result<()> {
    if !config.embedding.is_enabled() {
        bail!("Embedding provider is disabled. Set [embedding] provider in config.");
    }

    let provider = embedding::create_provider(&config.embedding)?;
    let model_name = provider.model_name().to_string();
    let pool = db::connect(config).await?;
    let batch_size = batch_size_override.unwrap_or(config.embedding.batch_size);

    // Delete all existing embeddings
    sqlx::query("DELETE FROM chunk_vectors")
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM embeddings").execute(&pool).await?;

    println!("embed rebuild — cleared existing embeddings");

    // Get all chunks
    let all_chunks = find_pending_chunks(&pool, &model_name, None).await?;

    if all_chunks.is_empty() {
        println!("  no chunks to embed");
        pool.close().await;
        return Ok(());
    }

    let total = all_chunks.len();
    let mut embedded = 0u64;
    let mut failed = 0u64;

    for batch in all_chunks.chunks(batch_size) {
        let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();

        match embedding::embed_texts(provider.as_ref(), &config.embedding, &texts).await {
            Ok(vectors) => {
                for (item, vec) in batch.iter().zip(vectors.iter()) {
                    let blob = embedding::vec_to_blob(vec);
                    upsert_embedding(
                        &pool,
                        &item.chunk_id,
                        &item.document_id,
                        &model_name,
                        provider.dims(),
                        &item.text_hash,
                        &blob,
                    )
                    .await?;
                    embedded += 1;
                }
            }
            Err(e) => {
                eprintln!("Warning: embedding batch failed: {}", e);
                failed += batch.len() as u64;
            }
        }
    }

    println!("embed rebuild");
    println!("  total chunks: {}", total);
    println!("  embedded: {}", embedded);
    println!("  failed: {}", failed);

    pool.close().await;
    Ok(())
}

/// Embed chunks during sync (inline). Non-fatal on failure.
pub async fn embed_chunks_inline(
    config: &Config,
    pool: &SqlitePool,
    chunks: &[crate::models::Chunk],
) -> (u64, u64) {
    if !config.embedding.is_enabled() {
        return (0, 0);
    }

    let provider = match embedding::create_provider(&config.embedding) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Warning: could not create embedding provider: {}", e);
            return (0, chunks.len() as u64);
        }
    };

    let model_name = provider.model_name().to_string();
    let mut embedded = 0u64;
    let mut pending = 0u64;

    for batch in chunks.chunks(config.embedding.batch_size) {
        // Check which chunks need embedding
        let mut need_embedding = Vec::new();
        for chunk in batch {
            let text_hash = hash_text(&chunk.text);
            let existing: Option<String> =
                sqlx::query_scalar("SELECT hash FROM embeddings WHERE chunk_id = ? AND model = ?")
                    .bind(&chunk.id)
                    .bind(&model_name)
                    .fetch_optional(pool)
                    .await
                    .unwrap_or(None);

            if existing.as_deref() == Some(&text_hash) {
                // Already up to date
                embedded += 1;
                continue;
            }

            need_embedding.push((chunk, text_hash));
        }

        if need_embedding.is_empty() {
            continue;
        }

        let texts: Vec<String> = need_embedding.iter().map(|(c, _)| c.text.clone()).collect();

        match embedding::embed_texts(provider.as_ref(), &config.embedding, &texts).await {
            Ok(vectors) => {
                for ((chunk, text_hash), vec) in need_embedding.iter().zip(vectors.iter()) {
                    let blob = embedding::vec_to_blob(vec);
                    if let Err(e) = upsert_embedding(
                        pool,
                        &chunk.id,
                        &chunk.document_id,
                        &model_name,
                        provider.dims(),
                        text_hash,
                        &blob,
                    )
                    .await
                    {
                        eprintln!("Warning: failed to store embedding for {}: {}", chunk.id, e);
                        pending += 1;
                    } else {
                        embedded += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: embedding batch failed: {}", e);
                pending += need_embedding.len() as u64;
            }
        }
    }

    (embedded, pending)
}

struct PendingChunk {
    chunk_id: String,
    document_id: String,
    text: String,
    text_hash: String,
}

async fn find_pending_chunks(
    pool: &SqlitePool,
    model: &str,
    limit: Option<usize>,
) -> Result<Vec<PendingChunk>> {
    let limit_val = limit.unwrap_or(usize::MAX) as i64;

    // Chunks that either have no embedding or have a stale hash
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
    .fetch_all(pool)
    .await?;

    let results: Vec<PendingChunk> = rows
        .iter()
        .map(|row| {
            let text: String = row.get("text");
            let text_hash = hash_text(&text);
            PendingChunk {
                chunk_id: row.get("chunk_id"),
                document_id: row.get("document_id"),
                text,
                text_hash,
            }
        })
        .collect();

    Ok(results)
}

async fn upsert_embedding(
    pool: &SqlitePool,
    chunk_id: &str,
    document_id: &str,
    model: &str,
    dims: usize,
    text_hash: &str,
    blob: &[u8],
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

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
    .bind(text_hash)
    .execute(pool)
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
    .bind(document_id)
    .bind(blob)
    .execute(pool)
    .await?;

    Ok(())
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}
