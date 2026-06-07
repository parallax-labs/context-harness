# ADR-0021: Zvec as Embedded Storage

**Status:** Proposed
**Date:** 2026-06-07

## Context

Context Harness currently uses SQLite as its native embedded store. SQLite stores documents, chunks, checkpoints, FTS5 keyword indexes, embedding metadata, and vector BLOBs. Semantic search reads every vector BLOB and computes cosine similarity in Rust.

That architecture is intentionally local-first and zero-server, but it has two limitations:

- vector search is brute-force over all stored vectors;
- replacing SQLite now requires addressing more than vector search, because SQLite also provides persistence, FTS5/BM25 keyword search, checkpointing, and operational tooling.

The project already has a `Store` trait and an accepted workspace split that anticipates alternative storage backends. zvec is an embedded in-process vector database with persistent collections, schemas, scalar fields, scalar filtering, crash recovery, and vector indexes such as HNSW and Flat. It fits the local-first direction better than a networked vector service.

## Decision

Adopt **zvec** as the proposed replacement for SQLite-backed native storage, subject to the implementation plan in [DESIGN-0006](../design/0006-zvec-storage-migration.md).

The intended end state is:

- `ZvecStore` replaces `SqliteStore` as the app's default native store.
- zvec collections store documents, chunks, checkpoints, embedding metadata, and chunk vectors.
- zvec native vector search replaces brute-force scanning of `chunk_vectors`.
- Context Harness keeps the existing core hybrid scoring algorithm rather than delegating ranking fusion to zvec.
- Keyword search receives an explicit replacement, preferably an embedded lexical sidecar such as Tantivy if FTS5/BM25 quality must be preserved.
- SQLite code and the `sqlx` dependency are removed after migration tooling, packaging, and specs are updated.

This ADR is **Proposed**, not Accepted, because zvec packaging and keyword-search replacement still need validation before the SQLite ADRs can be superseded.

## Alternatives Considered

**Keep SQLite and add sqlite-vec.** This would preserve FTS5 and minimize migration work, but it does not satisfy the goal of replacing SQLite usage.

**Use zvec only for vectors, keep SQLite for documents and FTS.** This is a good incremental performance path, but it leaves two stores and keeps SQLite as the primary database.

**Use zvec for everything and drop keyword search.** This is the cleanest storage architecture, but it breaks a core retrieval mode and weakens hybrid search for exact-match queries.

**Use zvec plus Tantivy.** This removes SQLite while preserving strong local keyword search. It adds a second embedded index that must be kept consistent with zvec. This is the likely final approach if keyword quality remains a requirement.

**Use a networked vector database such as Qdrant, Weaviate, or PostgreSQL/pgvector.** These provide mature vector indexing and filtering, but they violate Context Harness's default local-first, zero-server installation model.

## Consequences

- Vector search can move from O(n) brute-force scans to indexed zvec queries.
- The storage abstraction must become complete enough for ingest, embeddings, stats, export, checkpoints, search, and get.
- Existing specs and runbooks that name SQLite/FTS5 must be revised once implementation behavior is locked.
- Accepted ADRs [ADR-0002](0002-sqlite-as-embedded-storage.md), [ADR-0003](0003-fts5-for-keyword-search.md), and [ADR-0004](0004-brute-force-vector-search.md) will need to be superseded if this decision is accepted and implemented.
- Release packaging becomes more complex because zvec uses a native library. Builds must avoid unpinned network downloads and verify all release targets.
- Existing user data needs a migration path from `ctx.sqlite` to the new zvec storage directory.

## References

- [DESIGN-0006: Zvec Storage Migration](../design/0006-zvec-storage-migration.md)
- zvec Rust crate docs: <https://docs.rs/zvec/latest/zvec/>
- zvec architecture overview: <https://zvec.org/en/blog/2026-04-29-zvec-architecture/>
