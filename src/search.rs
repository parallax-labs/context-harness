use anyhow::{bail, Result};
use chrono::NaiveDate;
use sqlx::Row;

use crate::config::Config;
use crate::db;

pub async fn run_search(
    config: &Config,
    query: &str,
    mode: &str,
    source_filter: Option<String>,
    since: Option<String>,
    limit: Option<i64>,
) -> Result<()> {
    if query.trim().is_empty() {
        println!("No results.");
        return Ok(());
    }

    match mode {
        "keyword" => {}
        "semantic" | "hybrid" => {
            bail!(
                "Mode '{}' requires embeddings (not available in Phase 1). Use --mode keyword.",
                mode
            );
        }
        _ => bail!(
            "Unknown search mode: {}. Use keyword, semantic, or hybrid.",
            mode
        ),
    }

    let pool = db::connect(config).await?;
    let final_limit = limit.unwrap_or(config.retrieval.final_limit);

    // Step 1: Get matching chunks from FTS5 with rank
    let fts_rows = sqlx::query(
        r#"
        SELECT chunk_id, document_id, rank, snippet(chunks_fts, 2, '>>>', '<<<', '...', 48) AS snippet
        FROM chunks_fts
        WHERE chunks_fts MATCH ?
        ORDER BY rank
        LIMIT ?
        "#,
    )
    .bind(query)
    .bind(final_limit * 5) // over-fetch to allow grouping
    .fetch_all(&pool)
    .await?;

    if fts_rows.is_empty() {
        println!("No results.");
        pool.close().await;
        return Ok(());
    }

    // Step 2: Group by document, pick best chunk per document
    use std::collections::HashMap;

    struct DocCandidate {
        doc_id: String,
        raw_score: f64,
        snippet: String,
    }

    let mut best_per_doc: HashMap<String, DocCandidate> = HashMap::new();

    for row in &fts_rows {
        let chunk_id: String = row.get("chunk_id");
        let doc_id: String = row.get("document_id");
        let rank: f64 = row.get("rank");
        let snippet: String = row.get("snippet");
        let raw_score = -rank; // negate so higher = better

        let _ = chunk_id; // used for FTS join but not needed here
        let entry = best_per_doc.entry(doc_id.clone());
        use std::collections::hash_map::Entry;
        match entry {
            Entry::Occupied(mut e) => {
                if raw_score > e.get().raw_score {
                    e.get_mut().raw_score = raw_score;
                    e.get_mut().snippet = snippet;
                }
            }
            Entry::Vacant(e) => {
                e.insert(DocCandidate {
                    doc_id,
                    raw_score,
                    snippet,
                });
            }
        }
    }

    let candidates: Vec<DocCandidate> = best_per_doc.into_values().collect();

    // Step 3: Fetch document metadata and apply filters
    struct DisplayResult {
        id: String,
        title: Option<String>,
        source: String,
        updated_at: i64,
        source_url: Option<String>,
        raw_score: f64,
        snippet: String,
    }

    let mut results: Vec<DisplayResult> = Vec::new();

    for cand in &candidates {
        let doc_row = sqlx::query(
            "SELECT id, title, source, source_id, updated_at, source_url FROM documents WHERE id = ?",
        )
        .bind(&cand.doc_id)
        .fetch_optional(&pool)
        .await?;

        if let Some(row) = doc_row {
            let source: String = row.get("source");
            let updated_at: i64 = row.get("updated_at");

            // Apply source filter
            if let Some(ref src) = source_filter {
                if &source != src {
                    continue;
                }
            }

            // Apply since filter
            if let Some(ref since_str) = since {
                let since_date = NaiveDate::parse_from_str(since_str, "%Y-%m-%d")?;
                let since_ts = since_date
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                if updated_at < since_ts {
                    continue;
                }
            }

            results.push(DisplayResult {
                id: row.get("id"),
                title: row.get("title"),
                source,
                updated_at,
                source_url: row.get("source_url"),
                raw_score: cand.raw_score,
                snippet: cand.snippet.clone(),
            });
        }
    }

    if results.is_empty() {
        println!("No results.");
        pool.close().await;
        return Ok(());
    }

    // Step 4: Sort by score desc, then updated_at desc, then id asc
    results.sort_by(|a, b| {
        b.raw_score
            .partial_cmp(&a.raw_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.id.cmp(&b.id))
    });

    // Truncate to final_limit
    results.truncate(final_limit as usize);

    // Step 5: Normalize scores to [0, 1]
    let raw_scores: Vec<f64> = results.iter().map(|r| r.raw_score).collect();
    let s_min = raw_scores.iter().cloned().fold(f64::INFINITY, f64::min);
    let s_max = raw_scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    for (i, result) in results.iter().enumerate() {
        let norm_score = if (s_max - s_min).abs() < f64::EPSILON {
            1.0
        } else {
            (result.raw_score - s_min) / (s_max - s_min)
        };

        let title_display = result.title.as_deref().unwrap_or("(untitled)");
        let date = chrono::DateTime::from_timestamp(result.updated_at, 0)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default();

        println!(
            "{}. [{:.2}] {} / {}",
            i + 1,
            norm_score,
            result.source,
            title_display
        );
        println!("    updated: {}", date);
        println!("    source: {}", result.source);
        if let Some(ref url) = result.source_url {
            println!("    url: {}", url);
        }
        println!(
            "    excerpt: \"{}\"",
            result.snippet.replace('\n', " ").trim()
        );
        println!("    id: {}", result.id);
        println!();
    }

    pool.close().await;
    Ok(())
}
