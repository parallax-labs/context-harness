+++
title = "Search & Retrieval"
description = "Keyword, semantic, and hybrid search modes explained."
weight = 9

[extra]
sidebar_label = "Search & Retrieval"
sidebar_group = "Reference"
sidebar_order = 9
+++

Context Harness supports three search modes, each suited to different query types.

## Keyword Search (FTS5)

SQLite FTS5 with BM25 ranking. No embeddings required. Finds exact and partial term matches.

```bash
$ ctx search "HttpClient interceptor" --mode keyword
```

**Good for:** known identifiers, function names, error messages, exact phrases.

## Semantic Search (Vector)

Cosine similarity over embeddings. Requires an embedding provider configured. Finds conceptually related content even when terms don't match.

```bash
$ ctx search "how to handle authentication" --mode semantic
```

**Good for:** natural language questions, conceptual queries, "how to" questions.

## Hybrid Search

Merges keyword and semantic results using Reciprocal Rank Fusion (RRF). The `hybrid_alpha` config value controls the blend:

- `0.0` = pure keyword
- `1.0` = pure semantic
- `0.6` = default — semantic-leaning blend

```bash
$ ctx search "deployment runbook" --mode hybrid
```

**Good for:** most queries — combines precision of keyword search with recall of semantic search.

## Tuning Parameters

```toml
[retrieval]
final_limit = 12           # Max results returned to caller
hybrid_alpha = 0.6          # Keyword ↔ semantic blend weight
candidate_k_keyword = 80    # How many FTS candidates to fetch
candidate_k_vector = 80     # How many vector candidates to fetch
group_by = "document"       # Group chunks by parent document
doc_agg = "max"             # Use max chunk score as document score
max_chunks_per_doc = 3      # Limit chunks shown per document
```

## Document Grouping

By default, results are grouped by document — if multiple chunks from the same document match, the highest-scoring chunk represents the document. The `max_chunks_per_doc` setting controls how many representative chunks are shown.

## Scoring

Scores are normalized to the 0–1 range. For hybrid search, the final score is computed via RRF:

> **RRF(d)** = α · rank_vector(d)⁻¹ + (1−α) · rank_keyword(d)⁻¹

See [HYBRID_SCORING.md](https://github.com/parallax-labs/context-harness/blob/main/docs/HYBRID_SCORING.md) for the full algorithm specification.

