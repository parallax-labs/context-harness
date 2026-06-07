# DESIGN-0006: Vector Index Acceleration

**Status:** Draft
**Date:** 2026-06-07
**Author:** pjones / Codex
**Related:** [ADR-0002](../adr/0002-sqlite-as-embedded-storage.md), [ADR-0003](../adr/0003-fts5-for-keyword-search.md), [ADR-0004](../adr/0004-brute-force-vector-search.md), [ADR-0018](../adr/0018-store-abstraction-and-workspace-split.md), [ADR-0021](../adr/0021-vector-index-acceleration.md), [SPEC-0002](../spec/0002-workspace-refactor.md), [SPEC-0003](../spec/0003-hybrid-scoring.md), [SPEC-0005](../spec/0005-usage-contract.md), [SPEC-0012](../spec/0012-storage-and-vector-index-interfaces.md)

## Context

Context Harness currently uses SQLite for the native store:

- normalized documents and chunks
- sync checkpoints
- SQLite FTS5 keyword search over chunk text
- embedding metadata and vector BLOB storage
- brute-force vector search in Rust over rows read from `chunk_vectors`
- database stats, export, `ctx init`, `ctx sync`, `ctx embed`, `ctx search`, and `ctx get`

SQLite and FTS5 are still valuable general-purpose infrastructure for this product. SQLite gives a zero-server local store, a single file to back up or delete, WAL-based concurrent reads during serving, idempotent migrations, and broad cross-platform support. FTS5 gives BM25 keyword retrieval in the same transactional store as chunks. Exact keyword search remains important for developer context queries: symbols, paths, error messages, config keys, and API names often need lexical matching that semantic search alone misses.

The performance concern is narrower: semantic search currently scans every stored vector, decodes every BLOB, computes cosine similarity in Rust, sorts all candidates, and then returns top-k. That path is intentionally simple and has been acceptable for small personal corpora, but early local experimentation suggests it can become slow in practice as chunk counts grow.

This design therefore narrows the previous "replace SQLite with zvec" idea into a proof-driven vector acceleration project:

- keep SQLite as the canonical document, chunk, checkpoint, FTS, stats, and export store;
- keep FTS5 as the canonical keyword retrieval implementation;
- benchmark the current brute-force vector path;
- evaluate zvec, sqlite-vec, or another embedded vector index as an optional accelerator;
- only replace storage layers if evidence shows the narrower accelerator approach is insufficient.

## Proposal

Add two prerequisite boundaries behind the existing CLI/search flow while preserving SQLite/FTS5 as the canonical store.

The intended shape is:

1. SQLite remains the source of truth for documents, chunks, checkpoints, embedding metadata, and keyword search.
2. The current `chunk_vectors` table remains a compatibility and migration source at first.
3. An app-level `AppStore` wraps native SQLite operations that are outside the reusable core search trait.
4. A new optional `VectorIndex` owns only vector candidate retrieval.
5. `SqliteStore::vector_search` remains functionally equivalent in the prerequisite PR and continues to use the exact brute-force SQLite scan by default.
6. If a future vector accelerator is unavailable, stale, or disabled, the system falls back to the current brute-force SQLite vector search.
7. Hybrid scoring remains owned by `context-harness-core` and continues to follow [SPEC-0003](../spec/0003-hybrid-scoring.md).

This avoids a premature full storage migration while still testing the part of the architecture that appears slow.

## Prerequisite Tracks

The first implementation work is intentionally not zvec and not sqlite-vec. It creates stable seams so later acceleration work can be evaluated without changing public behavior.

### Track A: AppStore

`AppStore` is an app-crate boundary for native operational storage:

- initialization/migrations;
- connector checkpoints;
- canonical document upserts from `SourceItem`;
- canonical chunk replacement;
- embedding metadata/vector maintenance;
- stats aggregation;
- export DTOs.

`AppStore` starts as `SqliteAppStore` backed by the existing `SqlitePool` and delegates reusable retrieval methods to the existing `SqliteStore`. This makes SQLite ownership explicit without moving native details into `context-harness-core`.

### Track B: VectorIndex

`VectorIndex` is an optional candidate-retrieval boundary for semantic search:

- `DisabledVectorIndex` reports disabled/unavailable and produces no candidates.
- `BruteForceSqliteVectorIndex` wraps the current exact SQLite vector scan and establishes the compatibility baseline.
- Later zvec or sqlite-vec backends must return `ChunkCandidate` values that core hybrid scoring can consume unchanged.

The prerequisite PR wires this interface only where behavior remains identical. The intended product experience is plug-and-play: default `backend = "auto"` should use zvec when the binary supports it and fall back to SQLite when it does not.

## Alternatives Considered

### Keep current SQLite brute-force vector search

This keeps the system simple and preserves all current behavior. It is acceptable if benchmarked latency remains below an agreed threshold for realistic corpora.

The downside is O(n) search cost over chunk vectors. The initial probe shows that vector search dominates keyword search at 10k chunks, so doing nothing should be a conscious decision backed by thresholds.

### zvec as an optional vector accelerator

zvec is an embedded in-process vector database with persistent collections, scalar filtering, WAL/crash recovery, and vector indexes such as HNSW and Flat. It may fit the local-first model while providing indexed vector search.

Benefits:

- purpose-built vector indexes instead of brute-force scans;
- persistent embedded storage rather than a network service;
- scalar filtering by source/date may map well to Context Harness filters.

Risks:

- native-library packaging and release-target support must be proven;
- zvec's Rust bundled build path may download native artifacts, which is not acceptable for reproducible release builds unless pinned or vendored;
- if used only as an index sidecar, consistency with SQLite must be managed.

### sqlite-vec as an optional vector accelerator

sqlite-vec would keep SQLite as the single storage engine and may allow vector search without introducing a separate sidecar database. The current vector BLOB format was already chosen for forward compatibility with sqlite-vec.

Benefits:

- preserves SQLite/FTS5 and one-store ergonomics;
- no separate document/vector consistency model;
- likely smaller conceptual change than zvec.

Risks:

- native SQLite extension packaging may still complicate release targets;
- performance and index support must be benchmarked against zvec and brute force;
- may not provide the same ANN/index capabilities as a dedicated vector engine, depending on available features.

### Full zvec storage replacement

This removes SQLite entirely and stores documents, chunks, checkpoints, and vectors in zvec, with a separate lexical replacement for FTS5.

Rejected for now. It solves more than the proven problem, removes useful SQLite/FTS5 behavior, requires a lexical sidecar such as Tantivy to preserve keyword quality, and creates a much larger migration surface.

### Semantic-only retrieval

This removes the need for FTS5 and simplifies the vector architecture.

Rejected. Keyword search is part of the current usage contract and remains important for developer workflows.

## Proposal Details

### Vector Index Interface

Introduce an app-level vector index abstraction rather than widening `context-harness-core::Store` immediately:

```rust
#[async_trait::async_trait]
pub trait VectorIndex: Send + Sync {
    async fn upsert(&self, record: VectorRecord<'_>) -> anyhow::Result<()>;
    async fn delete_chunk(&self, chunk_id: &str) -> anyhow::Result<()>;
    async fn delete_document(&self, document_id: &str) -> anyhow::Result<()>;
    async fn search(&self, query: &[f32], opts: VectorSearchOptions<'_>) -> anyhow::Result<Vec<ChunkCandidate>>;
    async fn health(&self) -> anyhow::Result<VectorIndexHealth>;
}
```

`VectorRecord` should include `chunk_id`, `document_id`, vector, model, dims, content hash, source, updated timestamp, and snippet text. Duplicating source/date/snippet metadata lets the vector index serve filtered candidates without rejoining SQLite for every vector hit.

### SQLite Remains Canonical

The vector index is an accelerator, not the authority.

- `documents`, `chunks`, `checkpoints`, `chunks_fts`, `embeddings`, and `chunk_vectors` remain the canonical schema during this design.
- `ctx export` and `ctx stats` continue reading SQLite unless a future spec changes that.
- `ctx embed rebuild` can rebuild the vector index from SQLite.
- If index corruption or version mismatch is detected, users can rebuild the index without re-syncing connectors.

### Search Flow

Keyword mode:

1. Query SQLite FTS5.
2. Return keyword candidates exactly as today.

Semantic mode:

1. Embed the query.
2. If a vector accelerator is enabled and healthy, query it.
3. Otherwise run current SQLite brute-force vector search.

Hybrid mode:

1. Query SQLite FTS5 for keyword candidates.
2. Query the vector accelerator, or brute-force fallback, for semantic candidates.
3. Pass both candidate sets through the existing core hybrid scoring algorithm.

The design should not use backend-specific hybrid fusion unless it can be proven to match [SPEC-0003](../spec/0003-hybrid-scoring.md).

### Performance Proof Plan

Use the ignored performance probe in `crates/context-harness/tests/perf_sqlite_store.rs` to establish SQLite baselines and compare candidate backends under the same synthetic corpus shape.

Run the current SQLite baseline with:

```bash
cargo test -p context-harness --test perf_sqlite_store -- --ignored --nocapture
```

The probe accepts environment overrides:

```bash
CTX_PERF_DOCS=5000 \
CTX_PERF_CHUNKS_PER_DOC=20 \
CTX_PERF_DIMS=384 \
CTX_PERF_REPEAT=5 \
CTX_PERF_CANDIDATE_K=80 \
cargo test -p context-harness --test perf_sqlite_store -- --ignored --nocapture
```

Run a scaling profile with comma-separated `docs x chunks_per_doc x dims` scenarios:

```bash
CTX_PERF_SCENARIOS=1000x10x384,5000x20x384,10000x20x384 \
CTX_PERF_REPEAT=5 \
cargo test -p context-harness --test perf_sqlite_store perf_sqlite_scaling_profile -- --ignored --nocapture
```

Use `CTX_PERF_OUTPUT=jsonl` when the output should be captured and compared by scripts:

```bash
CTX_PERF_OUTPUT=jsonl \
cargo test -p context-harness --test perf_sqlite_store -- --ignored --nocapture
```

Track at least:

- corpus size: documents, chunks, dimensions, database/storage bytes
- populate time
- `keyword_search` min/median/average/max
- `vector_search` min/median/average/max
- `hybrid_search` min/median/average/max
- candidate count (`candidate_k`)

Initial local baseline on 2026-06-07 with 1,000 documents, 10 chunks per document, 384 dimensions, and 5 repeats:

| Path | Median |
|------|--------|
| `keyword_search` | 12.95 ms |
| `vector_search` | 193.09 ms |
| `hybrid_search` | 293.13 ms |

These numbers are not acceptance thresholds because developer hardware varies, but they show that the vector path dominates keyword at 10k chunks. A future zvec or sqlite-vec prototype should run the same corpus shape and report both latency and recall against this baseline.

### Candidate Acceptance Thresholds

Before enabling a vector accelerator by default, define concrete targets. Suggested starting targets:

- 10k chunks, 384 dims: semantic median under 50 ms.
- 100k chunks, 384 dims: semantic median under 150 ms.
- Recall: top-10 overlap against brute-force Flat/exact search at or above 0.95 for the synthetic probe and any real eval dataset.
- Keyword: unchanged, because FTS5 remains canonical.
- Hybrid: output shape and scoring explanation remain compatible with [SPEC-0003](../spec/0003-hybrid-scoring.md).
- Packaging: release builds do not download unpinned native artifacts.

These are draft targets, not authoritative spec requirements. They should be adjusted after real corpus measurements.

### Config

Keep existing configs working. Add optional vector index settings rather than changing `[db]` semantics:

```toml
[vector_index]
backend = "auto"       # "auto" | "zvec" | "sqlite" | "disabled" | "sqlite-vec"
path = "auto"          # resolves beside the SQLite app store in the app data root
metric = "cosine"
index = "hnsw"         # backend-specific; "flat" for exact/debug mode
fallback = "sqlite"    # use brute-force SQLite if the accelerator is unhealthy
```

The desired default layout is:

```text
<app-data-root>/
  ctx.sqlite
  vector-index/
```

SQLite remains the source of truth. The zvec directory is derived state and can be removed or rebuilt without re-syncing connectors.

Bootstrap and maintenance commands should make this feel automatic:

```bash
ctx init                  # initializes SQLite and any auto-supported vector sidecar
ctx vector-index status   # reports backend, health, freshness, and fallback mode
ctx vector-index rebuild  # rebuilds the sidecar from SQLite embeddings
```

Explicit overrides are still useful for debugging, deterministic baselines, and packaging fallout:

- `backend = "auto"`: use zvec when available, otherwise SQLite fallback.
- `backend = "zvec"`: require zvec and error if it cannot initialize.
- `backend = "sqlite"` or `"disabled"`: force the current brute-force path.

Resolved questions:

- Should this live under `[retrieval.vector_index]` instead of top-level `[vector_index]`?
  - Keep it top-level because it has storage lifecycle, sidecar path, and rebuild behavior.
- Should the default stay `"disabled"` until one backend has release-grade packaging?
  - No. The desired product behavior is `auto` with SQLite fallback. Normal builds may still omit zvec support until CI proves the native packaging story.
- Should `path` default beside `db.path`, e.g. `ctx.sqlite.vector-index/`?
  - Use `path = "auto"` and resolve it under the same app data root as the SQLite file.

### Implementation Plan

1. **Keep the benchmark probe.**
   - Land `perf_sqlite_store.rs`.
   - Run it on several corpus sizes: 10k, 50k, 100k chunks.
   - Capture local and CI-machine baselines if possible.

2. **Add the AppStore prerequisite.**
   - Define `AppStore`, `PendingChunk`, `StoreStats`, `SourceStats`, `ExportData`, `ExportDocument`, and `ExportChunk`.
   - Add `SqliteAppStore` over the existing SQLite pool.
   - Move checkpoint, pending embedding, clear embedding, stats, export, and source-item write SQL behind it without changing output.

3. **Add the VectorIndex prerequisite.**
   - Define `VectorIndex`, `VectorRecord`, `VectorSearchOptions`, and `VectorIndexHealth`.
   - Add a disabled/no-op backend and a brute-force SQLite adapter for parity.
   - Keep `SqliteStore::vector_search` functionally equivalent.

4. **Later: refactor vector writes through one path.**
   - Keep writing `chunk_vectors`.
   - Also upsert/delete into the configured vector index from `replace_chunks`, `upsert_embedding`, and `embed rebuild`.
   - Add `ctx vector-index rebuild` or equivalent if the index is sidecar-backed.

5. **Bake off zvec.**
   - Add zvec behind an optional `zvec-bundled` Cargo feature.
   - Add an ignored benchmark that builds a zvec sidecar from SQLite vectors and compares latency, index build time, sidecar size, and top-k overlap against the exact SQLite scan.
   - Run the feature in CI/release target jobs to prove native packaging.
   - Implement HNSW first and Flat mode if the Rust API exposes it cleanly.
   - Preserve source/date filtering.
   - Benchmark latency and recall against the brute-force baseline.

6. **Later: prototype sqlite-vec backend if packaging looks better.**
   - Compare against zvec on the same probe.
   - Prefer the backend with the best balance of latency, recall, packaging, and complexity.

7. **Decide default behavior.**
   - Keep `auto` as the product default.
   - Let normal builds fall back to SQLite if they lack zvec support.
   - Enable zvec in release builds only after CI proves supported targets build and the bake-off clears latency/recall thresholds.

## Acceptance Criteria

- The ignored SQLite performance probe compiles and can be run locally.
- The probe prints corpus shape, storage size, populate time, and search timings.
- Current normal CI behavior is unchanged because the probe is ignored.
- `AppStore` wraps app-level SQLite operations while preserving sync, embed, stats, and export output.
- `VectorIndex` exists with disabled and brute-force SQLite baseline implementations.
- The top-level `[vector_index]` config defaults to auto, falls back to SQLite, and existing configs that omit it continue to load.
- A zvec bake-off benchmark exists behind an opt-in Cargo feature.
- A future zvec/sqlite-vec prototype can be compared against the same benchmark shape.
- SQLite/FTS5 remains the canonical keyword store.
- The design does not require changing the current usage contract unless later benchmarks justify a larger migration.

## Risks

- **Benchmark realism:** Synthetic corpora may not match real developer corpora. Mitigation: add real eval datasets once available.
- **Sidecar consistency:** Optional vector indexes can become stale relative to SQLite. Mitigation: health checks, rebuild command, and brute-force fallback.
- **Packaging risk:** zvec or sqlite-vec may not fit the release matrix. Mitigation: keep acceleration optional until proven.
- **Recall regression:** ANN indexes can return approximate neighbors. Mitigation: compare against brute-force exact search and keep Flat/exact mode for verification.
- **Scope creep:** A vector accelerator project can turn into a full storage rewrite. Mitigation: keep SQLite replacement explicitly out of scope until benchmarks prove a broader need.

## Dependencies

- Existing `Store` abstraction and `SqliteStore`.
- Benchmark probe in `crates/context-harness/tests/perf_sqlite_store.rs`.
- Candidate backend investigation for zvec and sqlite-vec.
- CI jobs that build the zvec feature on supported release targets.
- Optional future retrieval eval dataset from [PRD-0009](../prd/0009-retrieval-quality-and-dogfooding.md).

## Open Questions

- What corpus size and latency threshold makes brute-force vector search unacceptable?
- Is zvec or sqlite-vec easier to package across the current release targets?
- Should the vector accelerator be a sidecar directory, an SQLite extension, or both depending on platform?
- What recall threshold is acceptable for semantic and hybrid search?
- Should vector-index health appear in `ctx stats`?
- Should `ctx embed rebuild` also rebuild the vector index, or should there be a dedicated command?
