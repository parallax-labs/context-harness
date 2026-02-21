use anyhow::Result;
use sqlx::Row;

use crate::config::Config;
use crate::db;

pub async fn run_get(config: &Config, id: &str) -> Result<()> {
    let pool = db::connect(config).await?;

    // Fetch document
    let doc_row = sqlx::query(
        "SELECT id, source, source_id, source_url, title, author, created_at, updated_at, content_type, body, metadata_json FROM documents WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?;

    let doc_row = match doc_row {
        Some(row) => row,
        None => {
            eprintln!("Error: document not found: {}", id);
            std::process::exit(1);
        }
    };

    let title: Option<String> = doc_row.get("title");
    let source: String = doc_row.get("source");
    let source_id: String = doc_row.get("source_id");
    let source_url: Option<String> = doc_row.get("source_url");
    let author: Option<String> = doc_row.get("author");
    let created_at: i64 = doc_row.get("created_at");
    let updated_at: i64 = doc_row.get("updated_at");
    let content_type: String = doc_row.get("content_type");
    let body: String = doc_row.get("body");
    let metadata_json: String = doc_row.get("metadata_json");

    println!("--- Document ---");
    println!("id:           {}", id);
    println!("title:        {}", title.as_deref().unwrap_or("(untitled)"));
    println!("source:       {}", source);
    println!("source_id:    {}", source_id);
    if let Some(ref url) = source_url {
        println!("source_url:   {}", url);
    }
    if let Some(ref auth) = author {
        println!("author:       {}", auth);
    }
    println!("created_at:   {}", format_ts(created_at));
    println!("updated_at:   {}", format_ts(updated_at));
    println!("content_type: {}", content_type);
    println!("metadata:     {}", metadata_json);
    println!();

    // Fetch chunks
    let chunk_rows = sqlx::query(
        "SELECT chunk_index, text FROM chunks WHERE document_id = ? ORDER BY chunk_index ASC",
    )
    .bind(id)
    .fetch_all(&pool)
    .await?;

    println!("--- Body ---");
    println!("{}", body);
    println!();

    println!("--- Chunks ({}) ---", chunk_rows.len());
    for row in &chunk_rows {
        let idx: i64 = row.get("chunk_index");
        let text: String = row.get("text");
        println!("[chunk {}]", idx);
        println!("{}", text);
        println!();
    }

    pool.close().await;
    Ok(())
}

fn format_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}
