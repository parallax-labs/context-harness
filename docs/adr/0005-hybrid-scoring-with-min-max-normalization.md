# ADR-0005: Hybrid Scoring with Min-Max Normalization

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness supports both keyword search (FTS5/BM25) and semantic search
(cosine similarity over embeddings). Each produces scores on a different
scale:

- BM25 scores are negative floats (lower is better, e.g. -12.3)
- Cosine similarity scores are in [-1, 1] (higher is better)

Users benefit from a hybrid mode that combines both signals — keyword search
excels at exact-match queries (error codes, function names) while semantic
search handles conceptual queries (questions, paraphrases). The challenge is
combining scores from incompatible scales into a single ranked list.

The full specification is in `docs/HYBRID_SCORING.md`.

## Decision

Use **min-max normalization** to bring both score sets to `[0, 1]`, then
combine them with a configurable weight:

```
hybrid_score = (1 - alpha) * normalized_keyword + alpha * normalized_semantic
```

The complete pipeline:

1. Fetch `candidate_k_keyword` keyword results (BM25).
2. Fetch `candidate_k_vector` semantic results (cosine similarity).
3. Min-max normalize each set independently to `[0, 1]`.
4. Merge candidates: `score = (1 - alpha) * keyword + alpha * semantic`.
   Missing scores treated as 0.
5. Aggregate to document level using MAX over chunk scores.
6. Sort by score descending, then `updated_at` descending, then `id`
   ascending (deterministic tie-breaking).
7. Truncate to `final_limit`.

Default configuration: `hybrid_alpha = 0.6` (slightly favoring semantic),
`candidate_k_keyword = 50`, `candidate_k_vector = 50`, `final_limit = 12`.

## Alternatives Considered

**Reciprocal Rank Fusion (RRF).** A rank-based fusion method that avoids
score normalization by combining reciprocal ranks: `1 / (k + rank)`. Simpler
to implement but discards score magnitude — a highly relevant keyword hit
and a marginal one at adjacent ranks contribute nearly equally. Min-max
preserves score magnitude, which better reflects relevance differences.

**Learned ranking / reranker models.** Cross-encoder rerankers (e.g.
ColBERT, BGE-reranker) can re-score candidates with high accuracy. Deferred
because: (a) it adds a model dependency, (b) latency increases significantly,
and (c) the current hybrid approach has not been shown to be the bottleneck.
The system should be used and measured before adding a reranker.

**Score normalization via z-score.** Normalizing by mean and standard deviation
instead of min-max. Requires maintaining running statistics across queries or
computing per-query. Min-max is simpler and sufficient when both score sets
are already bounded.

**No hybrid — user picks a mode.** Users can already choose `keyword`,
`semantic`, or `hybrid` via the `--mode` flag. But having a good default
hybrid mode means most queries work well without the user needing to think
about which mode to use.

## Consequences

- Hybrid search produces consistently good results across both exact-match
  and conceptual queries without requiring users to choose a mode.
- The `hybrid_alpha` parameter is tunable per deployment. Teams with
  high-quality embeddings can increase alpha; teams without embeddings
  fall back to keyword-only automatically.
- Min-max normalization is per-query, so scores are not comparable across
  different queries. This is acceptable since search results are consumed
  per-query, not aggregated.
- The `--explain` flag on `ctx search` exposes raw keyword, semantic, and
  hybrid scores for debugging and tuning.
- Deterministic tie-breaking ensures reproducible result ordering, which
  aids testing and debugging.
