//! Search engine with keyword, semantic, and hybrid retrieval modes.
//!
//! The core search algorithm operates entirely through the [`Store`] trait,
//! with no database or configuration dependencies. The calling application
//! is responsible for embedding queries, constructing [`SearchParams`],
//! and passing the appropriate store implementation.
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

use anyhow::{bail, Result};
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::HashMap;

use crate::store::{ChunkCandidate, DocumentMetadata, Store};

/// Retrieval tuning parameters, decoupled from application config.
#[derive(Debug, Clone)]
pub struct SearchParams {
    /// Weight for semantic vs keyword: `hybrid = (1-α)*keyword + α*semantic`.
    pub hybrid_alpha: f64,
    /// Number of keyword candidates to fetch.
    pub candidate_k_keyword: i64,
    /// Number of vector candidates to fetch.
    pub candidate_k_vector: i64,
    /// Maximum results to return.
    pub final_limit: i64,
}

/// Bundles all inputs for a single search invocation.
#[derive(Debug, Clone)]
pub struct SearchRequest<'a> {
    /// Search query text.
    pub query: &'a str,
    /// Pre-computed query embedding (required for semantic/hybrid modes).
    pub query_vec: Option<&'a [f32]>,
    /// `"keyword"`, `"semantic"`, or `"hybrid"`.
    pub mode: &'a str,
    /// Only return results from this connector source.
    pub source_filter: Option<&'a str>,
    /// Only return documents updated after this date (`YYYY-MM-DD`).
    pub since: Option<&'a str>,
    /// Retrieval tuning parameters.
    pub params: SearchParams,
    /// If true, populate [`ScoreExplanation`] on each result.
    pub explain: bool,
}

/// A search result matching the `SCHEMAS.md` `context.search` response shape.
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
    /// Scoring breakdown (populated when `explain` is true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<ScoreExplanation>,
}

/// Scoring breakdown for a search result.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreExplanation {
    /// Normalized keyword score (0.0 if absent from keyword candidates).
    pub keyword_score: f64,
    /// Normalized semantic score (0.0 if absent from vector candidates).
    pub semantic_score: f64,
    /// The alpha weight used: `hybrid = (1-α)*keyword + α*semantic`.
    pub alpha: f64,
    /// Number of keyword candidates retrieved.
    pub keyword_candidates: usize,
    /// Number of vector candidates retrieved.
    pub vector_candidates: usize,
}

/// Run a hybrid search against a [`Store`] backend.
///
/// This is the core search function that all frontends (CLI, HTTP) delegate to.
/// It fetches candidates from the store, normalizes scores, merges, aggregates
/// by document, and returns sorted results.
pub async fn search<S: Store>(store: &S, req: &SearchRequest<'_>) -> Result<Vec<SearchResultItem>> {
    if req.query.trim().is_empty() {
        return Ok(Vec::new());
    }

    match req.mode {
        "keyword" | "semantic" | "hybrid" => {}
        _ => bail!(
            "Unknown search mode: {}. Use keyword, semantic, or hybrid.",
            req.mode
        ),
    }

    let keyword_candidates = if req.mode == "keyword" || req.mode == "hybrid" {
        store
            .keyword_search(
                req.query,
                req.params.candidate_k_keyword,
                req.source_filter,
                req.since,
            )
            .await?
    } else {
        Vec::new()
    };

    let vector_candidates = if req.mode == "semantic" || req.mode == "hybrid" {
        match req.query_vec {
            Some(qv) => {
                store
                    .vector_search(
                        qv,
                        req.params.candidate_k_vector,
                        req.source_filter,
                        req.since,
                    )
                    .await?
            }
            None => bail!("query_vec is required for semantic/hybrid mode"),
        }
    } else {
        Vec::new()
    };

    if keyword_candidates.is_empty() && vector_candidates.is_empty() {
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

    let effective_alpha = match req.mode {
        "keyword" => 0.0,
        "semantic" => 1.0,
        _ => req.params.hybrid_alpha,
    };

    struct ScoredChunk {
        document_id: String,
        hybrid_score: f64,
        keyword_score: f64,
        semantic_score: f64,
        snippet: String,
    }

    let kw_count = keyword_candidates.len();
    let vec_count = vector_candidates.len();

    let mut scored_chunks: Vec<ScoredChunk> = all_chunks
        .iter()
        .map(|(chunk_id, cand)| {
            let k = kw_map.get(chunk_id.as_str()).copied().unwrap_or(0.0);
            let v = vec_map.get(chunk_id.as_str()).copied().unwrap_or(0.0);
            let hybrid = (1.0 - effective_alpha) * k + effective_alpha * v;
            ScoredChunk {
                document_id: cand.document_id.clone(),
                hybrid_score: hybrid,
                keyword_score: k,
                semantic_score: v,
                snippet: cand.snippet.clone(),
            }
        })
        .collect();

    struct DocResult {
        doc_id: String,
        doc_score: f64,
        keyword_score: f64,
        semantic_score: f64,
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
                keyword_score: sc.keyword_score,
                semantic_score: sc.semantic_score,
                best_snippet: sc.snippet.clone(),
            });
        if sc.hybrid_score > entry.doc_score {
            entry.doc_score = sc.hybrid_score;
            entry.keyword_score = sc.keyword_score;
            entry.semantic_score = sc.semantic_score;
            entry.best_snippet = sc.snippet.clone();
        }
    }

    let mut results: Vec<SearchResultItem> = Vec::new();

    for doc_result in doc_map.values() {
        let meta: Option<DocumentMetadata> =
            store.get_document_metadata(&doc_result.doc_id).await?;

        if let Some(meta) = meta {
            if let Some(src) = req.source_filter {
                if meta.source != src {
                    continue;
                }
            }

            if let Some(since_str) = req.since {
                let since_date = NaiveDate::parse_from_str(since_str, "%Y-%m-%d")?;
                let since_ts = since_date
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                if meta.updated_at < since_ts {
                    continue;
                }
            }

            let updated_at_iso = format_ts_iso(meta.updated_at);

            let explanation = if req.explain {
                Some(ScoreExplanation {
                    keyword_score: doc_result.keyword_score,
                    semantic_score: doc_result.semantic_score,
                    alpha: effective_alpha,
                    keyword_candidates: kw_count,
                    vector_candidates: vec_count,
                })
            } else {
                None
            };

            results.push(SearchResultItem {
                id: meta.id,
                score: doc_result.doc_score,
                title: meta.title,
                source: meta.source,
                source_id: meta.source_id,
                updated_at: updated_at_iso,
                snippet: doc_result.best_snippet.clone(),
                source_url: meta.source_url,
                explain: explanation,
            });
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.id.cmp(&b.id))
    });

    results.truncate(req.params.final_limit as usize);

    Ok(results)
}

/// Format a Unix timestamp as ISO 8601.
pub fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Min-max normalize raw scores to `[0.0, 1.0]`.
///
/// If all scores are equal, they are normalized to `1.0`.
pub fn normalize_scores(candidates: &[ChunkCandidate]) -> Vec<(&ChunkCandidate, f64)> {
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
