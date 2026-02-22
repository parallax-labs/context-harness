+++
title = "Search & Retrieval"
description = "Keyword, semantic, and hybrid search modes, scoring, and tuning."
weight = 11

[extra]
sidebar_label = "Search & Retrieval"
sidebar_group = "Reference"
sidebar_order = 10
+++

Context Harness supports three search modes that can be mixed and tuned for your use case.

### Keyword search (FTS5/BM25)

Uses SQLite's FTS5 extension with BM25 ranking. Fast, zero-cost (no API calls), and works without embeddings.

```bash
$ ctx search "deployment procedure" --mode keyword
```

Good for: exact term matching, code symbols, error messages, specific identifiers.

### Semantic search

Vector similarity search over embeddings. Requires `[embedding]` to be configured and `ctx embed pending` to have been run.

```bash
$ ctx search "how to ship code to production" --mode semantic
```

Good for: natural language questions, conceptual queries, finding related content that uses different terminology.

### Hybrid search

Combines keyword and semantic search with weighted scoring. The `hybrid_alpha` parameter controls the mix:

```bash
$ ctx search "auth middleware" --mode hybrid
```

| `hybrid_alpha` | Behavior |
|-----------------|----------|
| `0.0` | 100% keyword (BM25 only) |
| `0.3` | Mostly keyword, some semantic |
| `0.6` | **Default** — balanced (recommended) |
| `0.8` | Mostly semantic |
| `1.0` | 100% semantic (vectors only) |

### How hybrid scoring works

1. **Candidate retrieval**: Fetch top `candidate_k_keyword` results from FTS5 and top `candidate_k_vector` from vector search
2. **Score normalization**: Both scores normalized to [0, 1] range via min-max scaling
3. **Weighted merge**: `final_score = (1 - alpha) * keyword_score + alpha * vector_score`
4. **Deduplication**: If the same chunk appears in both result sets, scores are merged
5. **Document grouping**: Chunks grouped by parent document, aggregated with `doc_agg` strategy
6. **Final ranking**: Top `final_limit` results returned

### Retrieval tuning

```toml
[retrieval]
final_limit = 12          # Max results returned
hybrid_alpha = 0.6        # 0.0 = keyword, 1.0 = semantic
candidate_k_keyword = 80  # FTS5 candidate pool size
candidate_k_vector = 80   # Vector candidate pool size
group_by = "document"     # Group chunks by parent doc
doc_agg = "max"           # Aggregation: "max" or "avg"
max_chunks_per_doc = 3    # Max chunks per doc in results
```

**Guidelines:**
- Increase `candidate_k_*` if you have a large corpus and want better recall
- Decrease `final_limit` for agent use (agents work better with fewer, more relevant results)
- Use `max_chunks_per_doc = 1` for broad coverage, higher for deep-dive queries
- `hybrid_alpha = 0.6` is a good starting point — adjust based on whether your queries are more keyword-heavy or conceptual

### CLI search

```bash
# Default keyword search
$ ctx search "error handling"

# Hybrid with source filter
$ ctx search "deploy" --mode hybrid --source git

# With custom limit
$ ctx search "config" --mode hybrid --limit 3
```

### API search

```bash
$ curl -s localhost:7331/tools/search \
    -H "Content-Type: application/json" \
    -d '{
      "query": "how to handle authentication",
      "mode": "hybrid",
      "limit": 5,
      "source": "git"
    }' | jq '.results[] | {title, score, source}'
```

### Client-side search (ctx-search.js)

For static sites, `ctx-search.js` provides a ⌘K search modal that runs entirely in the browser:

```html
<script src="/ctx-search.js"
    data-json="/data.json"
    data-trigger="#search-btn"
    data-placeholder="Search docs...">
</script>
```

Features:
- Zero dependencies, single `<script>` tag
- Loads a pre-built `data.json` index
- ⌘K / Ctrl+K keyboard shortcut
- Real-time fuzzy search with highlighted snippets
- Click-through to source URLs
- Dark theme, mobile responsive
