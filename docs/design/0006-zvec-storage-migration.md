# DESIGN-0006: Zvec Storage Migration

**Status:** Draft
**Date:** 2026-06-07
**Author:** pjones / Codex
**Related:** [ADR-0002](../adr/0002-sqlite-as-embedded-storage.md), [ADR-0003](../adr/0003-fts5-for-keyword-search.md), [ADR-0004](../adr/0004-brute-force-vector-search.md), [ADR-0018](../adr/0018-store-abstraction-and-workspace-split.md), [ADR-0021](../adr/0021-zvec-as-embedded-storage.md), [SPEC-0002](../spec/0002-workspace-refactor.md), [SPEC-0003](../spec/0003-hybrid-scoring.md), [SPEC-0005](../spec/0005-usage-contract.md)

## Context

Context Harness currently uses SQLite for every native storage concern:

- normalized documents and chunks
- sync checkpoints
- SQLite FTS5 keyword search over chunk text
- embedding metadata and vector BLOB storage
- brute-force vector search in Rust over rows read from `chunk_vectors`
- database stats, export, `ctx init`, `ctx sync`, `ctx embed`, `ctx search`, and `ctx get`

The project has already introduced a `Store` trait in `context-harness-core`, and core search goes through that trait. That makes the search path pluggable, but the native app still has direct `sqlx` usage in `ingest.rs`, `embed_cmd.rs`, `stats.rs`, `export.rs`, `migrate.rs`, and `db.rs`. Replacing SQLite with zvec therefore requires a storage migration, not only a semantic-search optimization.

The motivation for zvec is to keep Context Harness local-first and embedded while replacing SQLite's BLOB plus brute-force vector path with a native vector database. Current zvec documentation describes it as an embedded in-process vector database with persistent collections, schemas, scalar filtering, WAL/crash recovery, and vector indexes such as HNSW and Flat. The Rust crate exposes collection lifecycle, DDL, DML, DQL, async wrappers, and a bundled build path.

The main constraint is that SQLite currently provides FTS5/BM25. Zvec is not a drop-in FTS5 replacement. The design must decide how keyword and hybrid retrieval survive once SQLite is removed.

## Proposal

Replace the app's `SqliteStore` and direct SQLite helpers with a native `ZvecStore` backed by zvec collections. Keep the public CLI, MCP, config shape, and core hybrid scoring behavior stable where practical, but explicitly move the authoritative storage contract from SQLite tables to zvec collections.

The migration should happen in phases:

1. Add the zvec dependency and `ZvecStore` behind a feature flag.
2. Extend the core `Store` trait with operations that direct SQL users still need: checkpoint reads/writes, pending embedding discovery, embedding rebuild clearing, stats, and export iteration.
3. Move ingest, embed maintenance, stats, and export from direct `sqlx` calls to the store abstraction.
4. Make zvec the native default after compatibility and packaging are verified.
5. Remove SQLite code, config wording, runbook references, and SQLite-specific tests.

### Storage Layout

Use three zvec collections under a configured storage directory derived from the existing `[db].path` value during the transition.

| Collection | Primary key | Purpose |
|------------|-------------|---------|
| `documents` | `document_id` | Full document metadata, body, dedup hash, and source identity |
| `chunks` | `chunk_id` | Chunk text, chunk ordering, embedding metadata, and the `embedding` vector field |
| `checkpoints` | `source` | Per-connector sync cursor and update timestamp |

The `chunks` collection is the vector-search collection. It stores scalar fields needed for result filtering and assembly:

| Field | Type | Notes |
|-------|------|-------|
| `chunk_id` | string primary key | Current chunk UUID |
| `document_id` | string, indexed | Parent document UUID |
| `source` | string, indexed | Connector label such as `filesystem:docs` |
| `source_id` | string | Source-local document ID |
| `source_url` | string optional | Duplicated for search result assembly |
| `title` | string optional | Duplicated for search result assembly |
| `updated_at` | int64, indexed | Used for `--since` filtering and tie-breaking |
| `chunk_index` | int64 | Used to reconstruct ordered chunks |
| `text` | string | Used for snippets and lexical search fallback |
| `hash` | string | Chunk content hash |
| `embedding_model` | string optional, indexed | Staleness detection |
| `embedding_dims` | int64 optional | Validation |
| `embedding_hash` | string optional | Staleness detection |
| `embedding_created_at` | int64 optional | Stats and debugging |
| `embedding` | vector fp32 optional | Native zvec vector search |

The `documents` collection remains the source of truth for full bodies and raw metadata. Chunk-level fields intentionally duplicate search metadata to avoid an extra fetch during candidate retrieval and to let zvec scalar filters prune vector search by source and updated timestamp.

### Store Abstraction Changes

The current `Store` trait is sufficient for search and `get`, but not sufficient to replace SQLite throughout the app. Add an app-facing storage trait or extend `Store` with these operations:

| Operation | Replaces |
|-----------|----------|
| `get_checkpoint(source)` / `set_checkpoint(source, cursor)` | `ingest.rs` checkpoint SQL |
| `find_pending_chunks(model, limit)` | `embed_cmd.rs` stale embedding SQL |
| `clear_embeddings()` | `ctx embed rebuild` deletes from `embeddings` and `chunk_vectors` |
| `stats()` | `stats.rs` aggregate SQL |
| `iter_export_documents()` / `iter_export_chunks()` | `export.rs` SQL |
| `initialize()` | `migrate::run_migrations()` |

Keep `context-harness-core::Store` focused on reusable retrieval if widening it would make the WASM/core boundary awkward. In that case, define a native-only `AppStore` trait in `context-harness`:

```rust
#[async_trait::async_trait]
pub trait AppStore: context_harness_core::store::Store {
    async fn initialize(&self) -> anyhow::Result<()>;
    async fn get_checkpoint(&self, source: &str) -> anyhow::Result<Option<i64>>;
    async fn set_checkpoint(&self, source: &str, cursor: i64) -> anyhow::Result<()>;
    async fn find_pending_chunks(&self, model: &str, limit: Option<usize>) -> anyhow::Result<Vec<PendingChunk>>;
    async fn clear_embeddings(&self) -> anyhow::Result<()>;
    async fn stats(&self) -> anyhow::Result<StoreStats>;
    async fn export_index(&self) -> anyhow::Result<ExportData>;
}
```

This keeps core's existing search contract stable while eliminating app-level direct SQL.

### Keyword Search

SQLite FTS5 is the hardest behavior to replace. The recommended migration is:

1. Preserve the `keyword`, `semantic`, and `hybrid` modes.
2. Use zvec for all persistence and semantic/vector candidates.
3. Add an embedded lexical index module for keyword candidates.
4. Build the lexical index from `chunks.text` during `replace_chunks`.

The lexical index can be implemented two ways:

| Option | Design | Tradeoff |
|--------|--------|----------|
| Tantivy sidecar | Store a Tantivy index under the same data directory, keyed by `chunk_id` | Best FTS/BM25 replacement, but introduces a second local index and consistency work |
| In-process BM25 index persisted in zvec fields | Store token stats in zvec or rebuild an in-memory index at startup | Avoids a second on-disk index, but more custom code and poorer large-corpus startup behavior |

Use Tantivy if preserving keyword quality is a release requirement. Use a simple in-memory lexical index only for an initial zvec prototype, and mark keyword ranking as non-authoritative until a real BM25 index lands.

The search pipeline should not use zvec's Rust `HybridSearch` helper for Context Harness hybrid scoring until it can be proven to match [SPEC-0003](../spec/0003-hybrid-scoring.md). Context Harness already has a deterministic hybrid scoring contract: get keyword candidates, get vector candidates, min-max normalize each signal, weighted merge, aggregate by document, and tie-break deterministically. zvec should supply vector candidates; the existing core algorithm should keep owning fusion.

### Vector Search

`ZvecStore::vector_search` should query the `chunks` collection's `embedding` vector field with cosine metric and top-k equal to `candidate_k_vector`.

Use HNSW for the default production index:

- metric: cosine
- vector type: fp32
- dimensions: `config.embedding.dims` when known, otherwise provider-reported dims
- HNSW params: start with zvec defaults or explicit `M=16`, `ef_construction=200`, then benchmark

Support Flat as a test/debug mode to compare exact recall against HNSW and against the current brute-force SQLite implementation.

### Performance Proof Plan

Before making zvec the default, use the ignored performance probe in `crates/context-harness/tests/perf_sqlite_store.rs` to establish SQLite baselines and compare candidate backends under the same synthetic corpus shape.

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
cargo test -p context-harness --test perf_sqlite_store -- --ignored --nocapture
```

Track at least:

- corpus size: documents, chunks, dimensions, database/storage bytes
- populate time
- `keyword_search` min/median/average/max
- `vector_search` min/median/average/max
- `hybrid_search` min/median/average/max

Initial local baseline on 2026-06-07 with 1,000 documents, 10 chunks per document, 384 dimensions, and 5 repeats:

| Path | Median |
|------|--------|
| `keyword_search` | 12.95 ms |
| `vector_search` | 193.09 ms |
| `hybrid_search` | 293.13 ms |

These numbers are not acceptance thresholds because developer hardware varies, but they are enough to justify benchmarking the current brute-force vector path before committing to a full replacement. A future zvec prototype should run the same corpus shape and report recall/latency against this baseline.

### Config

Keep `[db] path = "./data/ctx.sqlite"` readable during migration for backward compatibility, but reinterpret it as a storage root when zvec is enabled. Add an optional backend selector:

```toml
[db]
backend = "zvec"          # "sqlite" during transition, then "zvec" default
path = "./data/ctx.zvec"  # directory for zvec collections and lexical sidecar

[retrieval]
vector_index = "hnsw"     # "hnsw" | "flat"
```

After SQLite removal, rename documentation language from "database path" to "storage path". Keep the key name `path` to avoid unnecessary config churn.

### Packaging

zvec's Rust crate supports several install paths, including explicit native library directories, `ZVEC_ROOT`, `ZVEC_LIB_DIR`, `pkg-config`, and a `bundled` feature that downloads a PyPI wheel at build time. Context Harness should not make release builds depend on network downloads.

Recommended packaging plan:

1. Add `zvec` as an optional dependency while prototyping.
2. Use `ZVEC_ROOT` or `ZVEC_LIB_DIR` in Nix/CI release builds.
3. Avoid enabling `zvec/bundled` in reproducible release builds unless the build process vendors or pins the artifact.
4. Validate all release targets before making zvec the default. Current docs.rs platform metadata only showed Linux targets for the crate; macOS and Windows release support must be verified before SQLite is removed.

### Module Changes

| Existing module | Change |
|-----------------|--------|
| `db.rs` | Replace SQLite pool creation with `store::open_app_store(config)` or remove after migration |
| `migrate.rs` | Become store initialization for zvec collections and lexical sidecar |
| `sqlite_store.rs` | Replace with `zvec_store.rs`; keep `SqliteStore` only behind transition feature |
| `ingest.rs` | Use `AppStore` for document upsert, chunk replacement, and checkpoints |
| `embed_cmd.rs` | Use `AppStore` for pending chunks, upsert embedding, and clearing embeddings |
| `search.rs` | Open configured store; pass store to core search |
| `get.rs` | Open configured store; call `Store::get_document` |
| `stats.rs` | Use `AppStore::stats` |
| `export.rs` | Use `AppStore::export_index` |
| `config.rs` | Add backend and zvec index options; update docs |
| `Cargo.toml` | Add optional zvec and optional lexical-index dependency |

## Alternatives Considered

### Keep SQLite for documents and FTS, use zvec only for vectors

This is the smallest migration: SQLite remains the document/checkpoint/FTS store and zvec replaces only `chunk_vectors`. It would improve semantic search while preserving keyword quality.

Rejected for this design because the user goal is to replace SQLite usage in the repo, not to add a second vector sidecar while keeping the SQLite database.

### zvec-only, no keyword replacement

This removes SQLite cleanly and makes semantic search fast, but breaks the current `keyword` mode and weakens `hybrid` mode. Exact-match queries for symbols, file names, and error strings are important to Context Harness.

Rejected unless the product intentionally narrows to semantic-only retrieval. That would require updating [SPEC-0005](../spec/0005-usage-contract.md), [SPEC-0003](../spec/0003-hybrid-scoring.md), and user-facing docs.

### zvec plus Tantivy lexical sidecar

This is the recommended full replacement if keyword quality must stay close to FTS5. It removes SQLite while keeping a strong local BM25 implementation.

The accepted downside is two embedded storage structures under one storage root: zvec collections plus a lexical index. The store layer must make chunk replacement atomic enough that a failed lexical update does not leave stale keyword results.

### zvec plus custom in-memory lexical index

This is attractive for a prototype and small corpora. It avoids adding Tantivy and can be rebuilt from zvec chunks at startup.

Rejected as the final design because startup cost and ranking quality become unclear as corpora grow. It can be a temporary phase if clearly labeled.

### sqlite-vec instead of zvec

This preserves SQLite and FTS5 while adding vector search inside SQLite.

Rejected for this request because it does not replace SQLite usage.

## Implementation Plan

1. **Add storage backend selection.**
   - Add `DbConfig.backend: Option<String>` with transition default `"sqlite"`.
   - Add a `storage` module with `open_app_store(config)`.
   - Keep all current SQLite behavior passing.

2. **Define native app storage operations.**
   - Add `AppStore` and DTOs for pending chunks, stats, and export.
   - Implement `AppStore` for the existing `SqliteStore` first.
   - Refactor `ingest`, `embed_cmd`, `stats`, and `export` to use `AppStore`.

3. **Introduce `ZvecStore`.**
   - Add zvec dependency behind a `zvec-store` feature.
   - Create/open the `documents`, `chunks`, and `checkpoints` collections.
   - Implement document/chunk/checkpoint operations.
   - Implement `get_document`, `get_document_metadata`, and `vector_search`.

4. **Prototype keyword replacement.**
   - Implement a temporary in-memory lexical index or Tantivy sidecar.
   - Wire `keyword_search` through the same `Store` method.
   - Compare top-k keyword results against SQLite/FTS5 on existing fixtures.

5. **Backfill and migration tooling.**
   - Add `ctx migrate sqlite-to-zvec` while SQLite is still available.
   - Read existing SQLite rows and write zvec collections plus lexical index.
   - Validate counts: documents, chunks, embeddings, checkpoints.

6. **Switch default backend.**
   - Change default backend to `"zvec"` after tests and packaging pass.
   - Keep SQLite behind a compatibility feature for one release if feasible.

7. **Remove SQLite.**
   - Remove `sqlx` dependency, `db.rs`, `migrate.rs` SQLite schema, `sqlite_store.rs`, and SQLite-specific docs.
   - Update config examples from `ctx.sqlite` to `ctx.zvec`.
   - Update accepted specs and write follow-up ADR/status changes.

## Acceptance Criteria

- `cargo test` passes with zvec enabled.
- `ctx init` creates zvec collections and the lexical index under the configured storage path.
- `ctx sync filesystem --full` ingests documents, chunks, checkpoints, and embeddings without creating a SQLite file.
- `ctx search --mode semantic` returns deterministic top-k results from zvec vector search.
- `ctx search --mode keyword` returns ranked keyword results with snippets.
- `ctx search --mode hybrid --explain` still reports keyword and semantic candidate counts and scores matching [SPEC-0003](../spec/0003-hybrid-scoring.md), unless that spec is intentionally revised.
- `ctx embed pending` detects missing and stale embeddings through the store abstraction.
- `ctx embed rebuild` clears and regenerates zvec embeddings.
- `ctx stats` and `ctx export` work without direct SQL.
- Existing integration tests no longer assume `data/ctx.sqlite` exists.
- Release builds do not download zvec artifacts from the network.

## Risks

- **Keyword quality regression:** zvec does not replace FTS5 by itself. Mitigation: use Tantivy or a measured lexical index, and keep SPEC-0003 fusion in core.
- **Packaging risk:** zvec's bundled build path downloads native artifacts. Mitigation: use pinned native libraries in Nix/CI and verify all target triples before removing SQLite.
- **Atomicity across zvec and lexical sidecar:** chunk replacement must not leave stale keyword candidates. Mitigation: write zvec first, update lexical second, and add repair/rebuild command for the lexical index.
- **Schema evolution:** zvec collections need explicit schema management. Mitigation: version collection schemas and add idempotent initialization similar to current migrations.
- **Data migration risk:** users have existing `.sqlite` stores. Mitigation: provide `ctx migrate sqlite-to-zvec`, verify counts, and document rollback during the transition release.
- **Spec drift:** current specs name SQLite and FTS5 as required behavior. Mitigation: keep this as a design doc until the behavior is locked, then update specs and ADRs in the same change as implementation.

## Dependencies

- zvec Rust crate and native zvec library packaging.
- A lexical-search decision: Tantivy sidecar versus custom zvec-backed index.
- CI/Nix support for zvec on supported release targets.
- Follow-up updates to [SPEC-0005](../spec/0005-usage-contract.md), [SPEC-0003](../spec/0003-hybrid-scoring.md), and storage runbooks once behavior is implemented.

## Open Questions

- Should keyword search be preserved at FTS5/BM25 quality, or is semantic-first retrieval acceptable for the zvec migration?
- Should the lexical replacement be Tantivy, a custom in-memory BM25 index, or a zvec-native feature if one becomes available?
- Which zvec index parameters should be the default for local corpora: HNSW defaults, explicit HNSW params, or Flat for exact recall at small sizes?
- Does zvec support every Context Harness release target with static or otherwise reproducible packaging?
- Should `[db].path` remain as a backward-compatible key forever, or should a new `[storage]` section be introduced in a breaking release?
- How long should the SQLite-to-zvec migration path be supported after zvec becomes default?

## References

- zvec Rust crate docs: <https://docs.rs/zvec/latest/zvec/>
- zvec architecture overview: <https://zvec.org/en/blog/2026-04-29-zvec-architecture/>
- zvec quickstart: <https://zvec.org/en/docs/db/quickstart/>
