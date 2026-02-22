//! Search engine with keyword, semantic, and hybrid retrieval modes.
//!
//! - **Keyword** — FTS5 full-text search using BM25 scoring.
//! - **Semantic** — Cosine similarity over stored embedding vectors.
//! - **Hybrid** — Weighted merge of keyword and semantic results using min-max
//!   normalization and configurable `hybrid_alpha` (see `docs/HYBRID_SCORING.md`).
//!
//! Results are returned as [`SearchResultItem`] values suitable for both CLI and
//! HTTP server consumption.
//!
//! # Hybrid Scoring Algorithm
//!
//! 1. Fetch `candidate_k_keyword` keyword candidates (BM25 rank).
//! 2. Fetch `candidate_k_vector` vector candidates (cosine similarity).
//! 3. Normalize both sets to `[0, 1]` using min-max normalization.
//! 4. Merge: `score = (1 - α) × keyword + α × semantic`.
//! 5. Group by document (MAX aggregation).
//! 6. Sort by score (desc), updated_at (desc), id (asc).
//! 7. Truncate to `final_limit`.
//!
//! # Filtering
//!
//! Results can be filtered by:
//! - `--source <name>` — only return documents from a specific connector
//! - `--since <YYYY-MM-DD>` — only return documents updated after a date

use anyhow::{bail, Result};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

use crate::config::Config;
use crate::db;
use crate::embedding;

/// A search result matching the `SCHEMAS.md` `context.search` response shape.
///
/// Used by both the CLI (`run_search`) and HTTP server (`handle_search`).
/// Scores are normalized to `[0.0, 1.0]` and sorted descending.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResultItem {
    /// Document UUID.
    pub id: String,
    /// Relevance score in `[0.0, 1.0]`.
    pub score: f64,
    /// Document title.
    pub title: Option<String>,
    /// Connector name.
    pub source: String,
    /// Identifier within the source.
    pub source_id: String,
    /// Last modification timestamp (ISO 8601).
    pub updated_at: String,
    /// Text excerpt from the best-matching chunk.
    pub snippet: String,
    /// Web-browsable URL, if available.
    pub source_url: Option<String>,
}

/// Core search function returning structured results.
///
/// This is the shared implementation used by both `ctx search` (CLI) and
/// `POST /tools/search` (HTTP server).
///
/// # Arguments
///
/// * `config` — Application configuration (retrieval tuning, embedding settings).
/// * `query` — Search query text.
/// * `mode` — Search mode: `"keyword"`, `"semantic"`, or `"hybrid"`.
/// * `source_filter` — Optional filter: only return results from this connector.
/// * `since` — Optional filter: only return documents updated after this date (`YYYY-MM-DD`).
/// * `limit` — Optional result limit (overrides `retrieval.final_limit`).
///
/// # Errors
///
/// - Empty query returns an empty result set (not an error).
/// - Unknown mode returns an error.
/// - Semantic/hybrid mode with disabled embeddings returns an error.
pub async fn search_documents(
    config: &Config,
    query: &str,
    mode: &str,
    source_filter: Option<&str>,
    since: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<SearchResultItem>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    match mode {
        "keyword" | "semantic" | "hybrid" => {}
        _ => bail!(
            "Unknown search mode: {}. Use keyword, semantic, or hybrid.",
            mode
        ),
    }

    if (mode == "semantic" || mode == "hybrid") && !config.embedding.is_enabled() {
        bail!(
            "Mode '{}' requires embeddings. Set [embedding] provider in config.",
            mode
        );
    }

    let pool = db::connect(config).await?;
    let final_limit = limit.unwrap_or(config.retrieval.final_limit);
    let alpha = config.retrieval.hybrid_alpha;

    let keyword_candidates = if mode == "keyword" || mode == "hybrid" {
        fetch_keyword_candidates(&pool, query, config.retrieval.candidate_k_keyword).await?
    } else {
        Vec::new()
    };

    let vector_candidates = if mode == "semantic" || mode == "hybrid" {
        fetch_vector_candidates(&pool, config, query, config.retrieval.candidate_k_vector).await?
    } else {
        Vec::new()
    };

    if keyword_candidates.is_empty() && vector_candidates.is_empty() {
        pool.close().await;
        return Ok(Vec::new());
    }

    let norm_keyword = normalize_scores(&keyword_candidates);
    let norm_vector = normalize_scores(&vector_candidates);

    let kw_map: HashMap<&str, f64> = norm_keyword
        .iter()
        .map(|(c, s)| (c.chunk_id.as_str(), *s))
        .collect();
    let vec_map: HashMap<&str, f64> = norm_vector
        .iter()
        .map(|(c, s)| (c.chunk_id.as_str(), *s))
        .collect();

    let mut all_chunks: HashMap<String, &ChunkCandidate> = HashMap::new();
    for c in &keyword_candidates {
        all_chunks.entry(c.chunk_id.clone()).or_insert(c);
    }
    for c in &vector_candidates {
        all_chunks.entry(c.chunk_id.clone()).or_insert(c);
    }

    let effective_alpha = match mode {
        "keyword" => 0.0,
        "semantic" => 1.0,
        _ => alpha,
    };

    struct ScoredChunk {
        document_id: String,
        hybrid_score: f64,
        snippet: String,
    }

    let mut scored_chunks: Vec<ScoredChunk> = all_chunks
        .iter()
        .map(|(chunk_id, cand)| {
            let k = kw_map.get(chunk_id.as_str()).copied().unwrap_or(0.0);
            let v = vec_map.get(chunk_id.as_str()).copied().unwrap_or(0.0);
            let hybrid = (1.0 - effective_alpha) * k + effective_alpha * v;
            ScoredChunk {
                document_id: cand.document_id.clone(),
                hybrid_score: hybrid,
                snippet: cand.snippet.clone(),
            }
        })
        .collect();

    struct DocResult {
        doc_id: String,
        doc_score: f64,
        best_snippet: String,
    }

    let mut doc_map: HashMap<String, DocResult> = HashMap::new();

    scored_chunks.sort_by(|a, b| {
        b.hybrid_score
            .partial_cmp(&a.hybrid_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for sc in &scored_chunks {
        let entry = doc_map
            .entry(sc.document_id.clone())
            .or_insert_with(|| DocResult {
                doc_id: sc.document_id.clone(),
                doc_score: sc.hybrid_score,
                best_snippet: sc.snippet.clone(),
            });
        if sc.hybrid_score > entry.doc_score {
            entry.doc_score = sc.hybrid_score;
            entry.best_snippet = sc.snippet.clone();
        }
    }

    let mut results: Vec<SearchResultItem> = Vec::new();

    for doc_result in doc_map.values() {
        let doc_row = sqlx::query(
            "SELECT id, title, source, source_id, updated_at, source_url FROM documents WHERE id = ?",
        )
        .bind(&doc_result.doc_id)
        .fetch_optional(&pool)
        .await?;

        if let Some(row) = doc_row {
            let source: String = row.get("source");
            let updated_at: i64 = row.get("updated_at");
            let source_id: String = row.get("source_id");

            if let Some(src) = source_filter {
                if source != src {
                    continue;
                }
            }

            if let Some(since_str) = since {
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

            let updated_at_iso = format_ts_iso(updated_at);

            results.push(SearchResultItem {
                id: row.get("id"),
                score: doc_result.doc_score,
                title: row.get("title"),
                source,
                source_id,
                updated_at: updated_at_iso,
                snippet: doc_result.best_snippet.clone(),
                source_url: row.get("source_url"),
            });
        }
    }

    // Sort: score desc, updated_at desc (string works for ISO8601), id asc
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.id.cmp(&b.id))
    });

    results.truncate(final_limit as usize);

    pool.close().await;
    Ok(results)
}

/// CLI entry point — calls [`search_documents`] and prints results to stdout.
pub async fn run_search(
    config: &Config,
    query: &str,
    mode: &str,
    source_filter: Option<String>,
    since: Option<String>,
    limit: Option<i64>,
) -> Result<()> {
    let results = search_documents(
        config,
        query,
        mode,
        source_filter.as_deref(),
        since.as_deref(),
        limit,
    )
    .await?;

    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }

    for (i, result) in results.iter().enumerate() {
        let title_display = result.title.as_deref().unwrap_or("(untitled)");
        println!(
            "{}. [{:.2}] {} / {}",
            i + 1,
            result.score,
            result.source,
            title_display
        );
        println!("    updated: {}", result.updated_at);
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

    Ok(())
}

/// Format a Unix timestamp as ISO 8601.
fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}

// ============ Candidate types ============

/// A candidate chunk from either keyword or vector search.
#[derive(Debug, Clone)]
struct ChunkCandidate {
    /// Chunk UUID.
    chunk_id: String,
    /// Parent document UUID.
    document_id: String,
    /// Raw score from the search engine (BM25 rank or cosine similarity).
    raw_score: f64,
    /// Text excerpt for display.
    snippet: String,
}

// ============ Keyword search ============

/// Fetch keyword search candidates using FTS5 with BM25 ranking.
///
/// Queries the `chunks_fts` virtual table and returns candidates
/// sorted by BM25 relevance. FTS5 returns negative rank values
/// (lower = better), which are negated to positive scores.
async fn fetch_keyword_candidates(
    pool: &SqlitePool,
    query: &str,
    candidate_k: i64,
) -> Result<Vec<ChunkCandidate>> {
    let rows = sqlx::query(
        r#"
        SELECT chunk_id, document_id, rank,
               snippet(chunks_fts, 2, '>>>', '<<<', '...', 48) AS snippet
        FROM chunks_fts
        WHERE chunks_fts MATCH ?
        ORDER BY rank
        LIMIT ?
        "#,
    )
    .bind(query)
    .bind(candidate_k)
    .fetch_all(pool)
    .await?;

    let candidates: Vec<ChunkCandidate> = rows
        .iter()
        .map(|row| {
            let rank: f64 = row.get("rank");
            ChunkCandidate {
                chunk_id: row.get("chunk_id"),
                document_id: row.get("document_id"),
                raw_score: -rank,
                snippet: row.get("snippet"),
            }
        })
        .collect();

    Ok(candidates)
}

// ============ Vector search ============

/// Fetch semantic search candidates using cosine similarity.
///
/// Embeds the query text, then computes cosine similarity against all
/// stored chunk vectors. Returns the top `candidate_k` results.
///
/// Note: This performs a brute-force scan over all vectors. For large
/// datasets, consider adding approximate nearest neighbor (ANN) indexing.
async fn fetch_vector_candidates(
    pool: &SqlitePool,
    config: &Config,
    query: &str,
    candidate_k: i64,
) -> Result<Vec<ChunkCandidate>> {
    let provider = embedding::create_provider(&config.embedding)?;
    let query_vec = embedding::embed_query(provider.as_ref(), &config.embedding, query).await?;

    let rows = sqlx::query(
        r#"
        SELECT cv.chunk_id, cv.document_id, cv.embedding,
               COALESCE(substr(c.text, 1, 240), '') AS snippet
        FROM chunk_vectors cv
        JOIN chunks c ON c.id = cv.chunk_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut candidates: Vec<ChunkCandidate> = rows
        .iter()
        .map(|row| {
            let blob: Vec<u8> = row.get("embedding");
            let vec = embedding::blob_to_vec(&blob);
            let similarity = embedding::cosine_similarity(&query_vec, &vec) as f64;
            ChunkCandidate {
                chunk_id: row.get("chunk_id"),
                document_id: row.get("document_id"),
                raw_score: similarity,
                snippet: row.get("snippet"),
            }
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.raw_score
            .partial_cmp(&a.raw_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(candidate_k as usize);

    Ok(candidates)
}

// ============ Score normalization ============

/// Min-max normalize raw scores to `[0.0, 1.0]`.
///
/// If all scores are equal, they are normalized to `1.0`.
///
/// # Formula
///
/// ```text
///              score - min
/// normalized = ───────────
///              max - min
/// ```
fn normalize_scores(candidates: &[ChunkCandidate]) -> Vec<(&ChunkCandidate, f64)> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let s_min = candidates
        .iter()
        .map(|c| c.raw_score)
        .fold(f64::INFINITY, f64::min);
    let s_max = candidates
        .iter()
        .map(|c| c.raw_score)
        .fold(f64::NEG_INFINITY, f64::max);

    candidates
        .iter()
        .map(|c| {
            let norm = if (s_max - s_min).abs() < f64::EPSILON {
                1.0
            } else {
                (c.raw_score - s_min) / (s_max - s_min)
            };
            (c, norm)
        })
        .collect()
}

// ============ Tests ============

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candidate(chunk_id: &str, doc_id: &str, score: f64) -> ChunkCandidate {
        ChunkCandidate {
            chunk_id: chunk_id.to_string(),
            document_id: doc_id.to_string(),
            raw_score: score,
            snippet: String::new(),
        }
    }

    #[test]
    fn test_normalize_empty() {
        let result = normalize_scores(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_normalize_single() {
        let candidates = vec![make_candidate("c1", "d1", 5.0)];
        let result = normalize_scores(&candidates);
        assert_eq!(result.len(), 1);
        assert!((result[0].1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_normalize_range() {
        let candidates = vec![
            make_candidate("c1", "d1", 10.0),
            make_candidate("c2", "d2", 5.0),
            make_candidate("c3", "d3", 0.0),
        ];
        let result = normalize_scores(&candidates);
        assert!((result[0].1 - 1.0).abs() < 1e-9);
        assert!((result[1].1 - 0.5).abs() < 1e-9);
        assert!((result[2].1 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_normalize_all_equal() {
        let candidates = vec![
            make_candidate("c1", "d1", 3.0),
            make_candidate("c2", "d2", 3.0),
        ];
        let result = normalize_scores(&candidates);
        for (_, score) in &result {
            assert!((*score - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn test_scores_always_in_unit() {
        let candidates = vec![
            make_candidate("c1", "d1", -5.0),
            make_candidate("c2", "d2", 100.0),
            make_candidate("c3", "d3", 42.0),
        ];
        let result = normalize_scores(&candidates);
        for (_, score) in &result {
            assert!(
                *score >= 0.0 && *score <= 1.0,
                "Score out of range: {}",
                score
            );
        }
    }

    #[test]
    fn test_hybrid_alpha_zero_equals_keyword() {
        let kw = vec![
            make_candidate("c1", "d1", 10.0),
            make_candidate("c2", "d2", 5.0),
            make_candidate("c3", "d3", 1.0),
        ];
        let vec_cands = vec![
            make_candidate("c1", "d1", 0.1),
            make_candidate("c2", "d2", 0.9),
        ];

        let norm_k = normalize_scores(&kw);
        let norm_v = normalize_scores(&vec_cands);

        let kw_map: HashMap<&str, f64> = norm_k
            .iter()
            .map(|(c, s)| (c.chunk_id.as_str(), *s))
            .collect();
        let vec_map: HashMap<&str, f64> = norm_v
            .iter()
            .map(|(c, s)| (c.chunk_id.as_str(), *s))
            .collect();

        let alpha = 0.0;
        let mut hybrid_scores: Vec<(&str, f64)> = Vec::new();
        let mut kw_only: Vec<(&str, f64)> = Vec::new();

        for c in &kw {
            let k = kw_map.get(c.chunk_id.as_str()).copied().unwrap_or(0.0);
            let v = vec_map.get(c.chunk_id.as_str()).copied().unwrap_or(0.0);
            let h = (1.0 - alpha) * k + alpha * v;
            hybrid_scores.push((c.chunk_id.as_str(), h));
            kw_only.push((c.chunk_id.as_str(), k));
        }

        hybrid_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        kw_only.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let h_order: Vec<&str> = hybrid_scores.iter().map(|(id, _)| *id).collect();
        let k_order: Vec<&str> = kw_only.iter().map(|(id, _)| *id).collect();
        assert_eq!(h_order, k_order, "alpha=0 should produce keyword ordering");
    }

    #[test]
    fn test_hybrid_alpha_one_equals_vector() {
        let kw = vec![
            make_candidate("c1", "d1", 10.0),
            make_candidate("c2", "d2", 5.0),
        ];
        let vec_cands = vec![
            make_candidate("c1", "d1", 0.1),
            make_candidate("c2", "d2", 0.9),
            make_candidate("c3", "d3", 0.5),
        ];

        let norm_k = normalize_scores(&kw);
        let norm_v = normalize_scores(&vec_cands);

        let kw_map: HashMap<&str, f64> = norm_k
            .iter()
            .map(|(c, s)| (c.chunk_id.as_str(), *s))
            .collect();
        let vec_map: HashMap<&str, f64> = norm_v
            .iter()
            .map(|(c, s)| (c.chunk_id.as_str(), *s))
            .collect();

        let alpha = 1.0;
        let mut hybrid_scores: Vec<(&str, f64)> = Vec::new();
        let mut vec_only: Vec<(&str, f64)> = Vec::new();

        for c in &vec_cands {
            let k = kw_map.get(c.chunk_id.as_str()).copied().unwrap_or(0.0);
            let v = vec_map.get(c.chunk_id.as_str()).copied().unwrap_or(0.0);
            let h = (1.0 - alpha) * k + alpha * v;
            hybrid_scores.push((c.chunk_id.as_str(), h));
            vec_only.push((c.chunk_id.as_str(), v));
        }

        hybrid_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        vec_only.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let h_order: Vec<&str> = hybrid_scores.iter().map(|(id, _)| *id).collect();
        let v_order: Vec<&str> = vec_only.iter().map(|(id, _)| *id).collect();
        assert_eq!(h_order, v_order, "alpha=1 should produce vector ordering");
    }
}
