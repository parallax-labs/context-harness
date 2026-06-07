# SPEC-0012: Storage and Vector Index Interfaces

**Status:** Authoritative
**Date:** 2026-06-07
**Scope:** App-level storage and optional vector-index interfaces in the native Context Harness application crate.

## Overview

This spec defines the storage boundary required before vector acceleration can be evaluated. SQLite/FTS5 remains canonical. Optional vector indexes MAY accelerate semantic candidate retrieval, but they MUST NOT become authoritative for documents, chunks, checkpoints, keyword search, stats, or export unless a later spec changes that contract.

## Definitions

**AppStore** is the app-crate storage interface for native operational storage behaviors outside the reusable core `Store` trait.

**SqliteAppStore** is the SQLite-backed implementation of `AppStore`.

**VectorIndex** is an optional interface for semantic vector candidate retrieval.

**ChunkCandidate** is the core search candidate shape consumed by semantic and hybrid scoring.

**Disabled vector index** is the default vector-index mode. It is unavailable by design and relies on SQLite fallback behavior.

## Requirements

1. `AppStore` SHALL own application storage initialization.
2. `AppStore` SHALL own connector checkpoint reads and writes.
3. `AppStore` SHALL own canonical document writes produced from connector `SourceItem` values.
4. `AppStore` SHALL own canonical chunk replacement for documents.
5. `AppStore` SHALL own embedding maintenance operations used by `ctx embed pending`, `ctx embed rebuild`, and inline sync embedding.
6. `AppStore` SHALL expose pending chunk discovery for missing or stale embeddings.
7. Pending chunk discovery SHALL return chunks with missing embedding rows for the requested model.
8. Pending chunk discovery SHALL return chunks whose stored embedding hash does not match the current chunk hash.
9. `AppStore` SHALL expose an embedding-clear operation that removes embedding metadata and vector rows while preserving documents and chunks.
10. `AppStore` SHALL expose stats DTOs with total document count, total chunk count, total embedded count, database size bytes, and per-source counts.
11. `AppStore` SHALL expose export DTOs matching the current `ctx export` JSON document and chunk shape.
12. `SqliteAppStore` SHALL be backed by the existing SQLite schema and `SqlitePool`.
13. `SqliteAppStore` SHALL preserve current CLI behavior, MCP behavior, config defaults, search ranking, export shape, and stats output.
14. `VectorIndex` SHALL own optional vector candidate retrieval only.
15. `VectorIndex` SHALL NOT own canonical document storage, chunk storage, checkpoint storage, FTS5 keyword search, stats, or export.
16. `VectorIndex::search` implementations SHALL return `ChunkCandidate` values compatible with core semantic and hybrid scoring.
17. `DisabledVectorIndex` SHALL report disabled and unavailable health.
18. Disabled vector-index configuration SHALL preserve current semantic and hybrid search behavior through SQLite fallback.
19. `BruteForceSqliteVectorIndex` SHALL return candidates with the same ordering and candidate shape as the current exact SQLite vector scan.
20. `SqliteStore::vector_search` SHALL remain functionally equivalent during the prerequisite implementation.
21. The default top-level `[vector_index]` configuration SHALL be:

```toml
[vector_index]
backend = "disabled"
path = "./data/vector-index"
metric = "cosine"
index = "flat"
fallback = "sqlite"
```

22. Config files that omit `[vector_index]` SHALL load with the default disabled configuration.
23. No zvec, sqlite-vec, or new native vector-index dependency is required by this prerequisite spec.

## Acceptance Criteria

- `cargo test -p context-harness-core` passes.
- `cargo test -p context-harness` passes.
- Tests cover `SqliteAppStore` checkpoint set/get round trip.
- Tests cover pending chunk discovery for missing and stale embeddings.
- Tests cover clearing embeddings while preserving documents and chunks.
- Tests cover export DTOs matching the current `ctx export` JSON shape.
- Tests cover stats document, chunk, and embedded counts.
- Tests cover `BruteForceSqliteVectorIndex` ordering parity with `SqliteStore::vector_search`.
- Tests cover disabled vector-index defaults and health.
- Ignored SQLite performance benchmarks remain available for baseline and scaling evaluation.
