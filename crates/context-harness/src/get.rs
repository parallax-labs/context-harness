//! Document retrieval by ID.
//!
//! Fetches a full document and its associated chunks from the database
//! via [`SqliteStore`]. Used by both the `ctx get` CLI command and the
//! `POST /tools/get` HTTP endpoint.
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

use context_harness_core::store::Store;
#[allow(unused_imports)]
pub use context_harness_core::store::{ChunkResponse, DocumentResponse};

use crate::config::Config;
use crate::db;
use crate::sqlite_store::SqliteStore;

/// Retrieves a document by its UUID, including all associated chunks.
///
/// This is the core retrieval function used by both the CLI (`ctx get`)
/// and the HTTP server (`POST /tools/get`).
pub async fn get_document(config: &Config, id: &str) -> Result<DocumentResponse> {
    let pool = db::connect(config).await?;
    let store = SqliteStore::new(pool.clone());

    let result = store.get_document(id).await?;
    pool.close().await;

    match result {
        Some(doc) => Ok(doc),
        None => bail!("document not found: {}", id),
    }
}

/// CLI entry point for `ctx get <id>`.
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
