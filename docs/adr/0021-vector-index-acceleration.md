# ADR-0021: Vector Index Acceleration

**Status:** Proposed
**Date:** 2026-06-07

## Context

Context Harness currently uses SQLite as its native embedded store. SQLite stores documents, chunks, checkpoints, FTS5 keyword indexes, embedding metadata, and vector BLOBs. Semantic search reads every vector BLOB and computes cosine similarity in Rust.

SQLite and FTS5 still fit the product well:

- SQLite supports the local-first, zero-server installation model.
- FTS5 provides BM25 keyword search in the same store as chunk text.
- Keyword search remains important for exact developer queries such as symbols, paths, config keys, API names, and error messages.
- The existing usage contract explicitly describes SQLite storage and FTS5 keyword search.

The weak point is narrower than the full storage stack: vector search is brute-force. Initial local benchmark evidence from `perf_sqlite_store.rs` showed that at 10,000 chunks and 384 dimensions, keyword search was about 13 ms median while vector search was about 193 ms median and hybrid search about 293 ms median on one developer machine.

## Decision

Propose keeping SQLite/FTS5 as the canonical native store and evaluating an optional vector index accelerator for semantic search.

The intended direction is:

- SQLite remains authoritative for documents, chunks, checkpoints, embeddings metadata, FTS5, stats, and export.
- FTS5 remains authoritative for keyword search.
- A vector index backend, such as zvec or sqlite-vec, may accelerate semantic candidate retrieval.
- The current brute-force SQLite vector path remains available as a fallback and exact baseline.
- Context Harness keeps its existing hybrid scoring algorithm in core rather than delegating fusion to a backend-specific hybrid search API.
- A full SQLite replacement is out of scope until benchmarks prove that a narrower vector accelerator cannot meet the product need.

This ADR is **Proposed**, not Accepted, because the vector backend, thresholds, recall targets, and packaging story still need validation.

## Alternatives Considered

**Keep current brute-force SQLite vector search.** This is simplest and preserves all current behavior. It may remain the right default for small corpora, but initial benchmarks show it can dominate search latency at 10k chunks.

**Use zvec as a vector accelerator.** zvec is an embedded vector database with persistent collections and vector indexes. It may provide the desired latency improvement while preserving local-first behavior. Packaging and sidecar consistency need proof.

**Use sqlite-vec as a vector accelerator.** sqlite-vec may preserve the one-store SQLite architecture while improving vector search. It still needs native extension packaging and performance validation.

**Replace SQLite fully with zvec.** This was considered but rejected for now. It removes useful SQLite/FTS5 behavior, requires a new keyword-search implementation, and creates a much larger migration than the current evidence justifies.

**Drop keyword search and use semantic-only retrieval.** Rejected because exact lexical search is core to developer context retrieval.

## Consequences

- The project can investigate the observed vector bottleneck without destabilizing the rest of the storage stack.
- Existing SQLite/FTS5 benefits remain intact.
- The design now needs a vector-index abstraction, health checks, rebuild support, and fallback behavior.
- Candidate backends must be judged on latency, recall, packaging, and consistency complexity.
- If a backend proves useful, accepted ADRs [ADR-0002](0002-sqlite-as-embedded-storage.md), [ADR-0003](0003-fts5-for-keyword-search.md), and [ADR-0004](0004-brute-force-vector-search.md) do not need to be superseded wholesale; only ADR-0004 may need updating or supersession for the vector-search implementation.

## References

- [DESIGN-0006: Vector Index Acceleration](../design/0006-vector-index-acceleration.md)
- [ADR-0002: SQLite as Embedded Storage](0002-sqlite-as-embedded-storage.md)
- [ADR-0003: FTS5 for Keyword Search](0003-fts5-for-keyword-search.md)
- [ADR-0004: Brute-Force Vector Search with BLOB Storage](0004-brute-force-vector-search.md)
