# Context Harness — Implementation Design

This document guarantees that every CLI command and tool call defined in USAGE.md
is implemented via explicit modules and traits.

---

## Module Structure

### core/ (src/)

- `models.rs` — Document, Chunk, SourceItem structs
- `chunk.rs` — Paragraph-boundary chunker
- `ingest.rs` — Ingestion pipeline orchestration
- `config.rs` — Configuration parsing and validation
- `embedding.rs` — EmbeddingProvider trait, OpenAI/Disabled providers, cosine similarity

Guarantees:
- Document model exists
- Chunk model exists
- normalize(SourceItem) -> Document
- chunk(Document) -> Vec<Chunk>
- embed(Vec<String>) -> Vec<Vec<f32>> (via provider trait)
- cosine_similarity(a, b) -> f32
- vec_to_blob / blob_to_vec for storage

---

### connectors/ (src/)

- `connector_fs.rs` — Filesystem connector

Guarantees:
- Connector produces SourceItems
- Supports incremental sync via checkpoints

---

### storage/ (src/)

- `db.rs` — SQLite connection + WAL
- `migrate.rs` — Schema migrations

Guarantees:
- SqliteStore implements:
  - upsert_document
  - replace_chunks
  - set_checkpoint
  - get_checkpoint
  - search_keyword
  - get_document
  - upsert_embedding
  - find_pending_chunks

Database tables guaranteed:
- documents
- chunks
- checkpoints
- chunks_fts (FTS5)
- embeddings
- chunk_vectors

---

### retrieval/ (src/)

- `search.rs` — Keyword, semantic, and hybrid search

Guarantees:
- Keyword search (FTS5 with BM25)
- Semantic search (cosine similarity over stored vectors)
- Hybrid search (weighted merge per HYBRID_SCORING.md)
- Min-max score normalization to [0, 1]
- Document-level grouping (MAX aggregation)
- Deterministic tie-breaking (score desc, updated_at desc, id asc)
- `search_documents()` returns `Vec<SearchResultItem>` for reuse by CLI and server

---

### embedding/ (src/)

- `embedding.rs` — Provider trait and implementations
- `embed_cmd.rs` — embed pending / embed rebuild commands

Guarantees:
- EmbeddingProvider trait with model_name() and dims()
- DisabledProvider (returns error on embed)
- OpenAIProvider (API calls with retry/backoff)
- Inline embedding during sync (non-fatal)
- Staleness detection via SHA256 hash of chunk text

---

### server/ (src/)

- `server.rs` — Axum HTTP server (MCP-compatible)

Guarantees:
- `POST /tools/search` → context.search (SCHEMAS.md)
- `POST /tools/get` → context.get (SCHEMAS.md)
- `GET /tools/sources` → context.sources (SCHEMAS.md)
- `GET /health` → health check
- Error responses follow error schema (code + message)
- CORS enabled
- Structured error codes: bad_request, not_found, embeddings_disabled, internal

---

### interfaces/ (src/)

- `main.rs` — CLI with clap
- `get.rs` — Document retrieval (reusable `get_document()` + CLI printer)
- `sources.rs` — Connector listing (reusable `get_sources()` + CLI printer)

---

## Data Flow Guarantee

sync command must:

```
Connector -> SourceItem
SourceItem -> normalize() -> Document
Document -> upsert_document()
Document -> chunk()
Chunks -> replace_chunks()
Chunks -> embed() if enabled (non-fatal)
Checkpoint updated
```

---

## CLI-to-Module Mapping

| CLI Command | Module Responsibility |
|-------------|----------------------|
| init        | migrate::run_migrations() |
| sources     | sources::list_sources() |
| sync        | ingest::run_sync() |
| search      | search::run_search() |
| get         | get::run_get() |
| embed pending | embed_cmd::run_embed_pending() |
| embed rebuild | embed_cmd::run_embed_rebuild() |
| serve mcp   | server::run_server() |

## HTTP-to-Module Mapping

| Endpoint | Handler | Core Function |
|----------|---------|---------------|
| POST /tools/search | handle_search | search::search_documents() |
| POST /tools/get | handle_get | get::get_document() |
| GET /tools/sources | handle_sources | sources::get_sources() |
| GET /health | handle_health | — |

---

## Stability

If a CLI flag or tool schema changes, this document must be updated.

The public contract is defined by:
- USAGE.md
- SCHEMAS.md
- HYBRID_SCORING.md
