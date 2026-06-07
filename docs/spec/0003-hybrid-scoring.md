# Hybrid Scoring Spec (Keyword + Vector)

This document defines the ranking math for Context Harness hybrid retrieval.
Implementation MUST follow this spec for deterministic, testable behavior.

---

## Terminology

- **Document**: normalized unit (message, issue, file, etc.)
- **Chunk**: a slice of a document body used for indexing/embedding
- **Keyword retrieval**: SQLite FTS5 match over `chunks.text`
- **Vector retrieval**: similarity search over chunk embeddings
- **Hybrid**: weighted merge of keyword + vector signals

---

## Required Inputs

```toml
[retrieval]
hybrid_alpha = 0.6
candidate_k_keyword = 80
candidate_k_vector = 80
final_limit = 12
group_by = "document"
doc_agg = "max"
```

- `candidate_k_*` MUST be >= `final_limit`
- Alpha must be clamped to [0.0, 1.0]

---

## Retrieval Pipeline (Hybrid)

### Step 1: Get keyword candidates

Query FTS5, returns (chunk_id, document_id, rank).
BM25 returns negative values where lower = better.

```
keyword_raw = -bm25
```

Collect top `candidate_k_keyword` chunks.

### Step 2: Get vector candidates

Vector search returns (chunk_id, document_id, similarity).
Cosine similarity in [-1, 1].

Collect top `candidate_k_vector` chunks.

---

## Score Normalization

Min-Max normalization per query:

```
s_min = min(s_i)
s_max = max(s_i)

if s_max == s_min:
  norm_i = 1.0
else:
  norm_i = (s_i - s_min) / (s_max - s_min)
```

---

## Chunk-level Hybrid Score

```
k = K.unwrap_or(0.0)
v = V.unwrap_or(0.0)

hybrid = (1 - alpha) * k + alpha * v
```

---

## Document Aggregation

Default: MAX

```
doc_score = max(hybrid(chunk) for chunk in doc)
```

---

## Tie-breaking

1. Newer `updated_at` wins
2. Lexicographic `document_id` (stable final)

---

## Final Results

Return top `final_limit` documents with:
- `doc_score` in [0, 1]
- `snippet` from best chunk
- Document metadata

---

## Edge Cases

- No keyword results → k = 0.0 for all, ranking purely vector-based
- No vector results → v = 0.0 for all, ranking purely keyword-based
- Both empty → return empty results
- Very small candidate set → return all

---

## Validation Tests

1. Scores always in [0, 1]
2. alpha=1.0 → hybrid ordering equals vector ordering
3. alpha=0.0 → hybrid ordering equals keyword ordering
4. Deterministic tie-breaking is stable across runs
5. Missing-signal docs can still surface

