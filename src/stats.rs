//! Database statistics and health overview.
//!
//! Provides a quick summary of what's indexed: document counts, chunk counts,
//! embedding coverage, and per-source breakdowns. Used by `ctx stats` to give
//! confidence that syncs and embeddings are working as expected.

use anyhow::Result;
use sqlx::Row;

use crate::config::Config;
use crate::db;

/// Per-source breakdown of document and chunk counts.
struct SourceStats {
    source: String,
    doc_count: i64,
    chunk_count: i64,
    embedded_count: i64,
    last_sync_ts: Option<i64>,
}

/// Run the stats command: query the database and print a summary.
pub async fn run_stats(config: &Config) -> Result<()> {
    let pool = db::connect(config).await?;

    let total_docs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM documents")
            .fetch_one(&pool)
            .await?;

    let total_chunks: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chunks")
            .fetch_one(&pool)
            .await?;

    let total_embedded: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chunk_vectors")
            .fetch_one(&pool)
            .await?;

    let db_size = std::fs::metadata(&config.db.path)
        .map(|m| m.len())
        .unwrap_or(0);

    println!("Context Harness — Database Stats");
    println!("================================");
    println!();
    println!("  Database:    {}", config.db.path.display());
    println!("  Size:        {}", format_bytes(db_size));
    println!();
    println!("  Documents:   {}", total_docs);
    println!("  Chunks:      {}", total_chunks);
    println!(
        "  Embedded:    {} / {} ({}%)",
        total_embedded,
        total_chunks,
        if total_chunks > 0 {
            (total_embedded * 100) / total_chunks
        } else {
            0
        }
    );

    // Per-source breakdown
    let source_rows = sqlx::query(
        r#"
        SELECT
            d.source,
            COUNT(DISTINCT d.id) AS doc_count,
            COUNT(DISTINCT c.id) AS chunk_count,
            COUNT(DISTINCT cv.chunk_id) AS embedded_count
        FROM documents d
        LEFT JOIN chunks c ON c.document_id = d.id
        LEFT JOIN chunk_vectors cv ON cv.chunk_id = c.id
        GROUP BY d.source
        ORDER BY doc_count DESC
        "#,
    )
    .fetch_all(&pool)
    .await?;

    // Fetch checkpoint timestamps per source
    let checkpoint_rows = sqlx::query("SELECT source, updated_at FROM checkpoints")
        .fetch_all(&pool)
        .await?;

    let mut source_stats: Vec<SourceStats> = Vec::new();
    for row in &source_rows {
        let source: String = row.get("source");
        let last_sync_ts = checkpoint_rows
            .iter()
            .find(|cp| {
                let cp_source: String = cp.get("source");
                cp_source == source
            })
            .map(|cp| cp.get::<i64, _>("updated_at"));

        source_stats.push(SourceStats {
            source,
            doc_count: row.get("doc_count"),
            chunk_count: row.get("chunk_count"),
            embedded_count: row.get("embedded_count"),
            last_sync_ts,
        });
    }

    if !source_stats.is_empty() {
        println!();
        println!("  By source:");
        println!(
            "  {:<24} {:>6} {:>8} {:>10}   {}",
            "SOURCE", "DOCS", "CHUNKS", "EMBEDDED", "LAST SYNC"
        );
        println!("  {}", "-".repeat(76));

        for s in &source_stats {
            let sync_display = match s.last_sync_ts {
                Some(ts) => format_ts_relative(ts),
                None => "never".to_string(),
            };
            println!(
                "  {:<24} {:>6} {:>8} {:>10}   {}",
                s.source, s.doc_count, s.chunk_count, s.embedded_count, sync_display
            );
        }
    }

    println!();

    pool.close().await;
    Ok(())
}

/// Format a byte count as a human-readable string.
fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Format a Unix timestamp as a relative time string (e.g. "3 hours ago").
fn format_ts_relative(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let delta = now - ts;

    if delta < 0 {
        return format_ts_iso(ts);
    }

    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        let mins = delta / 60;
        format!("{} min{} ago", mins, if mins == 1 { "" } else { "s" })
    } else if delta < 86400 {
        let hours = delta / 3600;
        format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
    } else if delta < 86400 * 30 {
        let days = delta / 86400;
        format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
    } else {
        format_ts_iso(ts)
    }
}

fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}
