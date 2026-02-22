//! Document retrieval by ID.
//!
//! Fetches a full document and its associated chunks from the database.
//! Used by both the `ctx get` CLI command and `POST /tools/get` HTTP endpoint.

use anyhow::{bail, Result};
use serde::Serialize;
use sqlx::Row;

use crate::config::Config;
use crate::db;

/// Document response matching SCHEMAS.md `context.get` response shape.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_at: String, // ISO8601
    pub updated_at: String, // ISO8601
    pub content_type: String,
    pub body: String,
    pub metadata: serde_json::Value,
    pub chunks: Vec<ChunkResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChunkResponse {
    pub index: i64,
    pub text: String,
}

/// Core get function returning structured data (used by CLI and server).
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

/// CLI entry point — calls get_document and prints to stdout.
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

fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}
