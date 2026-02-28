# ADR-0002: SQLite as Embedded Storage

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness needs persistent storage for documents, chunks, full-text
search indexes, embedding vectors, and sync checkpoints. As a local-first
tool, it must work without requiring users to install, configure, or maintain
a separate database server.

Requirements:

- Zero-configuration: works out of the box with a single file path
- Full-text search capability (keyword retrieval is a core feature)
- Concurrent read access during HTTP serving
- Portable across Linux, macOS, and Windows
- Small footprint suitable for personal-scale corpora (thousands to tens of
  thousands of documents)

## Decision

Use **SQLite** via the `sqlx` crate with **WAL (Write-Ahead Logging)** mode
and a connection pool of up to 5 connections.

The schema consists of six tables:

| Table | Purpose |
|-------|---------|
| `documents` | Normalized documents with `(source, source_id)` unique key and `dedup_hash` |
| `chunks` | Text segments with `chunk_index`, `text`, and content `hash` |
| `chunks_fts` | FTS5 virtual table over chunk text for keyword search |
| `checkpoints` | Per-connector sync cursors for incremental ingestion |
| `embeddings` | Embedding metadata (model name, dimensions, content hash) |
| `chunk_vectors` | Embedding BLOBs (little-endian f32 arrays) |

Migrations are idempotent (`CREATE TABLE IF NOT EXISTS`) and run via
`ctx init` or automatically on first use.

## Alternatives Considered

**PostgreSQL.** Excellent full-text search and pgvector for embeddings, but
requires a running server. This violates the local-first, zero-configuration
constraint. Users would need Docker or a managed instance.

**DuckDB.** Good analytical performance and embeddable, but the Rust ecosystem
was less mature at decision time. No built-in FTS equivalent to FTS5. Better
suited for OLAP than the mixed read/write workload of sync + serve.

**Embedded key-value stores (sled, RocksDB, redb).** Fast for point lookups
but lack full-text search. Building FTS on top would require a separate
library (Tantivy) and additional index management, increasing complexity
with no clear benefit over SQLite's integrated FTS5.

**Flat files (JSON, MessagePack).** Simple but no indexing, no concurrent
access, no full-text search. Would require loading everything into memory
for search, which does not scale.

## Consequences

- Single `.sqlite` file is the entire data store — easy to back up, move,
  or delete.
- WAL mode allows concurrent reads during `ctx serve mcp` while sync writes
  proceed without blocking queries.
- SQLite's 281 TB size limit is far beyond the expected corpus size; scaling
  is not a practical concern for the target use case.
- FTS5 is available without additional dependencies, enabling keyword search
  from day one.
- Vector search must be implemented in application code (see
  [ADR-0004](0004-brute-force-vector-search.md)) since SQLite has no native
  vector index. This is acceptable at personal scale but would need
  revisiting for enterprise-scale deployments.
- The `sqlx` crate provides compile-time query checking (when enabled) and
  async access, fitting naturally with the tokio runtime.
