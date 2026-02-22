//! Document retrieval by ID.
//!
//! Fetches a full document and its associated chunks from the database.
//! Used by both the `ctx get` CLI command and the `POST /tools/get` HTTP endpoint.
//!
//! # Usage
//!
//! ```bash
//! # Retrieve a document by UUID
//! ctx get 550e8400-e29b-41d4-a716-446655440000
//! ```
//!
//! # Response Shape
//!
//! The response matches the `context.get` schema defined in `docs/SCHEMAS.md`,
//! including full document metadata, body text, and all chunks ordered by index.

use anyhow::{bail, Result};
use serde::Serialize;
use sqlx::Row;

use crate::config::Config;
use crate::db;

/// Full document response including metadata, body, and chunks.
///
/// This struct matches the `context.get` response shape defined in
/// `docs/SCHEMAS.md`. It is serialized as JSON by the HTTP server
/// and printed in a human-readable format by the CLI.
///
/// # Fields
///
/// All timestamps are formatted as ISO 8601 strings in UTC.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    /// The unique UUID of the document.
    pub id: String,
    /// The connector that ingested this document (e.g., `"filesystem"`, `"git"`, `"s3"`).
    pub source: String,
    /// The source-specific identifier (e.g., file path, S3 key).
    pub source_id: String,
    /// Optional URL pointing to the original source location.
    pub source_url: Option<String>,
    /// Optional human-readable title.
    pub title: Option<String>,
    /// Optional author attribution.
    pub author: Option<String>,
    /// Creation timestamp in ISO 8601 format.
    pub created_at: String,
    /// Last modification timestamp in ISO 8601 format.
    pub updated_at: String,
    /// MIME content type (e.g., `"text/plain"`, `"text/markdown"`).
    pub content_type: String,
    /// The full body text of the document.
    pub body: String,
    /// Additional metadata as a JSON object.
    pub metadata: serde_json::Value,
    /// Ordered list of text chunks derived from the document body.
    pub chunks: Vec<ChunkResponse>,
}

/// A single chunk within a document response.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkResponse {
    /// Zero-based index of this chunk within the document.
    pub index: i64,
    /// The text content of this chunk.
    pub text: String,
}

/// Retrieves a document by its UUID, including all associated chunks.
///
/// This is the core retrieval function used by both the CLI (`ctx get`)
/// and the HTTP server (`POST /tools/get`).
///
/// # Arguments
///
/// - `config` — application configuration (for database path).
/// - `id` — the UUID of the document to retrieve.
///
/// # Returns
///
/// A [`DocumentResponse`] containing the document's metadata, body, and chunks.
///
/// # Errors
///
/// Returns an error if the document is not found or a database error occurs.
pub async fn get_document(config: &Config, id: &str) -> Result<DocumentResponse> {
    let pool = db::connect(config).await?;

    let doc_row = sqlx::query(
        "SELECT id, source, source_id, source_url, title, author, created_at, updated_at, content_type, body, metadata_json FROM documents WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?;

    let doc_row = match doc_row {
        Some(row) => row,
        None => {
            pool.close().await;
            bail!("document not found: {}", id);
        }
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
    .fetch_all(&pool)
    .await?;

    let chunks: Vec<ChunkResponse> = chunk_rows
        .iter()
        .map(|row| ChunkResponse {
            index: row.get("chunk_index"),
            text: row.get("text"),
        })
        .collect();

    pool.close().await;

    Ok(DocumentResponse {
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
    })
}

/// CLI entry point for `ctx get <id>`.
///
/// Calls [`get_document`] and prints the result in a human-readable format
/// to stdout: metadata fields, then body text, then each chunk.
///
/// Exits with a non-zero status code if the document is not found.
pub async fn run_get(config: &Config, id: &str) -> Result<()> {
    let doc = match get_document(config, id).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("--- Document ---");
    println!("id:           {}", doc.id);
    println!(
        "title:        {}",
        doc.title.as_deref().unwrap_or("(untitled)")
    );
    println!("source:       {}", doc.source);
    println!("source_id:    {}", doc.source_id);
    if let Some(ref url) = doc.source_url {
        println!("source_url:   {}", url);
    }
    if let Some(ref auth) = doc.author {
        println!("author:       {}", auth);
    }
    println!("created_at:   {}", doc.created_at);
    println!("updated_at:   {}", doc.updated_at);
    println!("content_type: {}", doc.content_type);
    println!("metadata:     {}", doc.metadata);
    println!();

    println!("--- Body ---");
    println!("{}", doc.body);
    println!();

    println!("--- Chunks ({}) ---", doc.chunks.len());
    for chunk in &doc.chunks {
        println!("[chunk {}]", chunk.index);
        println!("{}", chunk.text);
        println!();
    }

    Ok(())
}

/// Formats a Unix timestamp (seconds since epoch) as an ISO 8601 string.
///
/// Falls back to the raw timestamp string if the conversion fails.
fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}
