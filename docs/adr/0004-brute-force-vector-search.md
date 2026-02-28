# ADR-0004: Brute-Force Vector Search with BLOB Storage

**Status:** Accepted
**Date:** Retroactive

## Context

Semantic search requires computing similarity between a query embedding and
all stored chunk embeddings. The system needs a vector storage and search
strategy that:

- Works within SQLite (no external vector database)
- Requires no native extensions that complicate cross-platform builds
- Supports the six-target build matrix (Linux x86/aarch64/musl, macOS
  x86/aarch64, Windows)
- Performs adequately at personal scale (thousands to low tens of thousands
  of chunks)

## Decision

Store embedding vectors as **little-endian f32 BLOBs** in the `chunk_vectors`
table. Perform vector search via **brute-force cosine similarity** computed
in Rust application code.

The search flow:

1. Embed the query text using the configured embedding provider.
2. Fetch all rows from `chunk_vectors` (or a filtered subset by source).
3. Decode each BLOB into `Vec<f32>`.
4. Compute cosine similarity between the query vector and each stored vector.
5. Sort by similarity descending and return the top-k results.

The BLOB format uses **little-endian f32** encoding, which is forward-compatible
with the `sqlite-vec` extension's expected format should it be adopted later.

## Alternatives Considered

**sqlite-vec extension.** A SQLite extension by Alex Garcia that adds native
vector search. It would eliminate the full table scan, but at decision time it
required loading a native shared library, which complicates distribution
(platform-specific `.so`/`.dylib` files, Nix packaging, musl compatibility).
The BLOB format was chosen to be compatible with sqlite-vec so it can be
adopted later without a data migration.

**pgvector (PostgreSQL).** Mature vector search with HNSW and IVFFlat indexes.
However, it requires PostgreSQL, which violates the embedded, local-first
constraint (see [ADR-0002](0002-sqlite-as-embedded-storage.md)).

**FAISS / Qdrant / Weaviate / Pinecone.** Dedicated vector databases or
libraries. Each adds an external dependency or service. FAISS requires C++
bindings; Qdrant requires a running server; cloud services require network
access. None fit the zero-dependency local-first model.

**Approximate Nearest Neighbor (ANN) in Rust.** Libraries like `hnsw_rs` or
`instant-distance` could build an in-memory ANN index. This would improve
search latency at scale but adds index build time, memory overhead, and
persistence complexity. Not justified at the current target scale.

## Consequences

- No native extensions or external services required — the binary is fully
  self-contained.
- Vector search is O(n) in the number of chunks. At 10,000 chunks with
  384-dimensional embeddings, this completes in single-digit milliseconds
  on modern hardware. The code includes a comment noting that ANN indexing
  should be evaluated if the corpus grows significantly.
- The little-endian f32 format is a deliberate forward-compatibility choice.
  Migrating to sqlite-vec later requires no data conversion — only adding
  the extension and switching the query path.
- Memory usage during search is proportional to corpus size since all vectors
  are loaded. For the target use case (personal knowledge bases, team docs),
  this is well within acceptable limits.
- Filtering by source is supported by joining with the `documents` table
  before loading vectors, reducing the scan set.
