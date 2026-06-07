+++
title = "Vector Search Bake-Off: SQLite Brute Force vs zvec"
description = "A real-corpus benchmark points to a simple architecture: SQLite remains the canonical app store, while zvec becomes a rebuildable vector sidecar."
date = 2026-06-07

[taxonomies]
tags = ["engineering", "benchmarks", "search"]
+++

Context Harness has used SQLite for everything local-first: documents, chunks, sync checkpoints, FTS5 keyword search, embedding metadata, and vector blobs. That has been a good tradeoff. SQLite is boring, portable, easy to back up, and excellent for the canonical app store.

The part that started looking suspicious was narrower: **semantic vector search**.

Today, semantic search reads every vector blob from SQLite, decodes it, computes cosine similarity in Rust, sorts the full candidate set, and returns top-k. That exact scan is simple and correct, but it is O(n) over stored vectors. We wanted evidence before changing the architecture, so we added a bake-off.

## The design direction

The shape that emerged is two stores, each doing the job it is good at:

```
app data root/
  ctx.sqlite       # canonical app store
  vector-index/    # derived zvec sidecar
```

SQLite stays canonical for:

- documents and chunks
- sync checkpoints
- FTS5 keyword search
- embedding metadata
- stats and export
- rebuild source for any vector sidecar

zvec becomes an optional, rebuildable vector index for semantic candidate retrieval. If it is missing, stale, or unavailable, Context Harness can fall back to the exact SQLite scan.

The intended user experience is plug-and-play:

```toml
[vector_index]
backend = "auto"
path = "auto"
metric = "cosine"
index = "hnsw"
fallback = "sqlite"
```

`auto` means: use zvec when the binary supports it and initialization succeeds; otherwise use the SQLite fallback. Users should not have to hand-wire a vector database just to get good local search.

## The real-corpus run

We ran the zvec benchmark against a real notes/code corpus:

```bash
CTX_PERF_CORPUS_ROOT=/Users/pjones/dev/rithum/Rithum \
CTX_PERF_DIMS=384 \
CTX_PERF_REPEAT=5 \
CTX_PERF_CANDIDATE_K=80 \
cargo test -p context-harness --features zvec-bundled \
  --test perf_zvec_vector_index perf_zvec_vector_index_real_corpus \
  -- --ignored --nocapture
```

Corpus shape:

| Metric | Value |
|--------|------:|
| Documents | 202 |
| Chunks | 4,917 |
| Source bytes | 12,696,305 |
| Dimensions | 384 |
| Candidate k | 80 |

Result:

| Path | Median | Average |
|------|-------:|--------:|
| SQLite exact vector scan | 112.56 ms | 128.13 ms |
| zvec HNSW sidecar | 0.77 ms | 0.80 ms |

Other measurements:

| Metric | Value |
|--------|------:|
| Top-k overlap vs SQLite exact scan | 0.988 |
| zvec build time | 250.61 ms |
| zvec optimize time | 235.87 ms |
| zvec sidecar size | 13,880,116 bytes |

That is a very clear signal: for this corpus shape, zvec candidate retrieval was roughly two orders of magnitude faster than the current brute-force scan, while preserving high top-k overlap against the exact baseline.

## Important caveat

This benchmark used deterministic vectors derived from chunk text, not provider-generated semantic embeddings. That means it proves the **index/search performance** on a real corpus shape, not end-to-end semantic quality.

That distinction matters. The next step is not "declare victory and delete SQLite vector search." The next step is to wire zvec behind the vector-index boundary, keep SQLite fallback, then run the same comparison with real embedding vectors and retrieval-quality checks.

## Why SQLite still stays

This result does not argue against SQLite. It argues against using SQLite as a brute-force vector scanner once corpora grow.

SQLite still gives Context Harness the right default app store:

- one local file for canonical data
- idempotent migrations
- WAL-backed local concurrency
- FTS5 keyword search for symbols, paths, errors, config keys, and exact phrases
- straightforward stats/export behavior
- a reliable rebuild source if the vector sidecar is deleted

FTS5 also remains important. Developer search is not semantic-only. Sometimes the right query is `Gearman`, `ORDER_STATUS_CANCELLED`, a file path, an endpoint name, or an error string. Keyword and vector retrieval are complementary.

## What changes next

The bake-off result supports the current implementation plan:

1. Keep SQLite and FTS5 canonical.
2. Keep the exact SQLite vector scan as fallback and recall baseline.
3. Add zvec as a derived vector sidecar behind `VectorIndex`.
4. Make `backend = "auto"` the ergonomic path.
5. Add `ctx vector-index status` and `ctx vector-index rebuild` so the sidecar is easy to inspect and repair.
6. Let CI prove zvec packaging across release targets before making it part of normal release binaries.

That gives us the best of both worlds: a boring canonical store, plus fast semantic candidate retrieval when the accelerator is available.

The nice thing about this result is not just that zvec is fast. It is that the architecture can stay simple: **SQLite owns truth; zvec owns speed.**
