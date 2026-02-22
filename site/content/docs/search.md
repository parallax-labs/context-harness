+++
title = "Search & Retrieval"
description = "How keyword, semantic, and hybrid search work under the hood."
weight = 9

[extra]
sidebar_label = "Search & Retrieval"
sidebar_group = "Reference"
sidebar_order = 9
+++

Context Harness supports three search modes. All modes return results normalized to a 0–1 score.

### Keyword search (FTS5)

SQLite FTS5 with BM25 ranking. No embeddings required. Matches exact and partial terms.

```bash
$ ctx search "HttpClient interceptor" --mode keyword
```

**Best for:** known identifiers, function names, error messages, exact phrases, grep-like queries.

### Semantic search (vector)

Cosine similarity over embeddings. Requires `[embedding]` configured. Finds conceptually related content even when terms don't match.

```bash
$ ctx search "how to handle authentication" --mode semantic
```

**Best for:** natural language questions, conceptual queries, "how do I..." questions.

### Hybrid search

Merges keyword and semantic results using Reciprocal Rank Fusion (RRF). The `hybrid_alpha` config controls the blend:

```
hybrid_alpha = 0.0  →  pure keyword
hybrid_alpha = 0.6  →  default (semantic-leaning)
hybrid_alpha = 1.0  →  pure semantic
```

```bash
$ ctx search "deployment runbook" --mode hybrid
```

**Best for:** most queries — combines keyword precision with semantic recall.

### How scoring works

For hybrid search, the final score is computed via RRF:

> **RRF(d)** = α · (k + rank_vector(d))⁻¹ + (1−α) · (k + rank_keyword(d))⁻¹

Documents are grouped by parent — if multiple chunks from the same document match, the highest-scoring chunk represents the document. This prevents a single long document from dominating results.

### Tuning

```toml
[retrieval]
final_limit = 12           # Max results returned
hybrid_alpha = 0.6          # Blend weight
candidate_k_keyword = 80    # FTS candidates before re-ranking
candidate_k_vector = 80     # Vector candidates before re-ranking
group_by = "document"       # Group chunks by document
doc_agg = "max"             # Use best chunk score as document score
max_chunks_per_doc = 3      # Max representative chunks per document
```
