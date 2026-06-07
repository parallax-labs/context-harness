//! Database statistics and health overview.
//!
//! Provides a quick summary of what's indexed: document counts, chunk counts,
//! embedding coverage, and per-source breakdowns. Used by `ctx stats` to give
//! confidence that syncs and embeddings are working as expected.

use anyhow::Result;

use crate::app_store::{AppStore, SqliteAppStore};
use crate::config::Config;

/// Run the stats command: query the database and print a summary.
pub async fn run_stats(config: &Config) -> Result<()> {
    let store = SqliteAppStore::connect(config).await?;
    let stats = store.stats().await?;

    println!("Context Harness — Database Stats");
    println!("================================");
    println!();
    println!("  Database:    {}", config.db.path.display());
    println!("  Size:        {}", format_bytes(stats.db_size_bytes));
    println!();
    println!("  Documents:   {}", stats.total_docs);
    println!("  Chunks:      {}", stats.total_chunks);
    println!(
        "  Embedded:    {} / {} ({}%)",
        stats.total_embedded,
        stats.total_chunks,
        if stats.total_chunks > 0 {
            (stats.total_embedded * 100) / stats.total_chunks
        } else {
            0
        }
    );

    if !stats.sources.is_empty() {
        println!();
        println!("  By source:");
        println!(
            "  {:<24} {:>6} {:>8} {:>10}   LAST SYNC",
            "SOURCE", "DOCS", "CHUNKS", "EMBEDDED"
        );
        println!("  {}", "-".repeat(76));

        for s in &stats.sources {
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

    store.close().await;
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
