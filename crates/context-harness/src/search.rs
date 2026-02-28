//! Search engine with keyword, semantic, and hybrid retrieval modes.
//!
//! This module provides the application-level entry points for search. The
//! core algorithm (normalization, hybrid merge, aggregation) lives in
//! `context-harness-core::search` and operates through the [`Store`] trait.
//! This wrapper handles config parsing, database connection, embedding, and
//! CLI output formatting.
//!
//! # Search Modes
//!
//! - **Keyword** — FTS5 full-text search using BM25 scoring.
//! - **Semantic** — Cosine similarity over stored embedding vectors.
//! - **Hybrid** — Weighted merge of keyword and semantic results.

use anyhow::{bail, Result};

#[allow(unused_imports)]
pub use context_harness_core::search::{
    normalize_scores, ScoreExplanation, SearchParams, SearchResultItem,
};
#[allow(unused_imports)]
pub use context_harness_core::store::ChunkCandidate;

use crate::config::Config;
use crate::db;
use crate::embedding;
use crate::sqlite_store::SqliteStore;

/// Core search function returning structured results.
///
/// This is the shared implementation used by both `ctx search` (CLI) and
/// `POST /tools/search` (HTTP server). Delegates to
/// `context_harness_core::search::search` via [`SqliteStore`].
pub async fn search_documents(
    config: &Config,
    query: &str,
    mode: &str,
    source_filter: Option<&str>,
    since: Option<&str>,
    limit: Option<i64>,
    explain: bool,
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
    let store = SqliteStore::new(pool.clone());

    let query_vec = if mode != "keyword" {
        let provider = embedding::create_provider(&config.embedding)?;
        Some(embedding::embed_query(provider.as_ref(), &config.embedding, query).await?)
    } else {
        None
    };

    let params = SearchParams {
        hybrid_alpha: config.retrieval.hybrid_alpha,
        candidate_k_keyword: config.retrieval.candidate_k_keyword,
        candidate_k_vector: config.retrieval.candidate_k_vector,
        final_limit: limit.unwrap_or(config.retrieval.final_limit),
    };

    let results = context_harness_core::search::search(
        &store,
        query,
        query_vec.as_deref(),
        mode,
        source_filter,
        since,
        &params,
        explain,
    )
    .await?;

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
    explain: bool,
) -> Result<()> {
    let results = search_documents(
        config,
        query,
        mode,
        source_filter.as_deref(),
        since.as_deref(),
        limit,
        explain,
    )
    .await?;

    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }

    if explain {
        if let Some(ex) = results.first().and_then(|r| r.explain.as_ref()) {
            println!(
                "Search: mode={}, alpha={:.2}, candidates: {} keyword + {} vector",
                mode, ex.alpha, ex.keyword_candidates, ex.vector_candidates
            );
            println!();
        }
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
        if let Some(ref ex) = result.explain {
            println!(
                "    scoring: keyword={:.3}  semantic={:.3}  → hybrid={:.3}",
                ex.keyword_score, ex.semantic_score, result.score
            );
        }
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
