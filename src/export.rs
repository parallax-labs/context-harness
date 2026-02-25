//! Export the search index as JSON for static site search.
//!
//! Produces a `data.json` file containing all documents and chunks,
//! suitable for use with `ctx-search.js` on static sites. Replaces
//! the Python one-liner previously used in `build-docs.sh`.

use anyhow::Result;
use serde::Serialize;
use sqlx::Row;
use std::path::Path;

use crate::config::Config;
use crate::db;

#[derive(Serialize)]
struct ExportData {
    documents: Vec<ExportDocument>,
    chunks: Vec<ExportChunk>,
}

#[derive(Serialize)]
struct ExportDocument {
    id: String,
    source: String,
    source_id: String,
    source_url: Option<String>,
    title: Option<String>,
    updated_at: i64,
    body: String,
}

#[derive(Serialize)]
struct ExportChunk {
    id: String,
    document_id: String,
    chunk_index: i64,
    text: String,
}

/// Export documents and chunks as JSON.
///
/// If `output` is `Some`, writes to that file path. Otherwise writes
/// to stdout for piping.
pub async fn run_export(config: &Config, output: Option<&Path>) -> Result<()> {
    let pool = db::connect(config).await?;

    let doc_rows = sqlx::query(
        "SELECT id, source, source_id, source_url, title, updated_at, body \
         FROM documents ORDER BY source_id",
    )
    .fetch_all(&pool)
    .await?;

    let chunk_rows = sqlx::query(
        "SELECT id, document_id, chunk_index, text \
         FROM chunks ORDER BY document_id, chunk_index",
    )
    .fetch_all(&pool)
    .await?;

    let documents: Vec<ExportDocument> = doc_rows
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

    let chunks: Vec<ExportChunk> = chunk_rows
        .iter()
        .map(|row| ExportChunk {
            id: row.get("id"),
            document_id: row.get("document_id"),
            chunk_index: row.get("chunk_index"),
            text: row.get("text"),
        })
        .collect();

    let doc_count = documents.len();
    let chunk_count = chunks.len();

    let data = ExportData { documents, chunks };
    let json = serde_json::to_string_pretty(&data)?;

    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &json)?;
            eprintln!(
                "Exported {} documents, {} chunks to {}",
                doc_count,
                chunk_count,
                path.display()
            );
        }
        None => {
            println!("{}", json);
        }
    }

    pool.close().await;
    Ok(())
}
