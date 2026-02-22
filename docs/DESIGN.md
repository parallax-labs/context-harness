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
- `connector_git.rs` — Git repository connector
- `connector_s3.rs` — Amazon S3 connector
- `connector_script.rs` — Lua scripted connectors (see `LUA_CONNECTORS.md`)

Guarantees:
- All connectors produce Vec<SourceItem>
- Supports incremental sync via checkpoints
- Git: clone/pull, subdirectory scoping, per-file git metadata, web URL generation
- S3: ListObjectsV2 with pagination, SigV4 signed requests, custom endpoint support
- Script: Lua 5.4 VM via `mlua`, sandboxed host APIs (http, json, fs, env, log)

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

### extensions/ (src/)

- `traits.rs` — `Connector` and `Tool` traits, `ToolContext`, `ConnectorRegistry`, `ToolRegistry`

Guarantees:
- `Connector` trait: `name()`, `description()`, `scan()` → `Vec<SourceItem>`
- `Tool` trait: `name()`, `description()`, `parameters_schema()`, `execute(params, ctx)`
- `ToolContext`: `search()`, `get()`, `sources()` bridging to core functions
- `ConnectorRegistry`: registered connectors available via `ctx sync custom:<name>`
- `ToolRegistry`: registered tools available via `POST /tools/{name}`

---

### server/ (src/)

- `server.rs` — Axum HTTP server (MCP-compatible)
- `tool_script.rs` — Lua tool loading, validation, execution (see `LUA_TOOLS.md`)
- `lua_runtime.rs` — Shared Lua VM setup + host APIs (used by connectors + tools)

Guarantees:
- `POST /tools/search` → context.search (SCHEMAS.md)
- `POST /tools/get` → context.get (SCHEMAS.md)
- `GET /tools/sources` → context.sources (SCHEMAS.md)
- `GET /tools/list` → tool discovery with OpenAI JSON schemas (LUA_TOOLS.md)
- `POST /tools/{name}` → dynamic Lua tool execution (LUA_TOOLS.md)
- `GET /health` → health check
- Error responses follow error schema (code + message)
- CORS enabled
- Structured error codes: bad_request, not_found, embeddings_disabled, tool_error, timeout, internal

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
| connector test | connector_script::test_script() |
| connector init | connector_script::scaffold_connector() |
| tool init   | tool_script::scaffold_tool() |
| tool test   | tool_script::test_tool() |
| tool list   | tool_script::list_tools() |

## HTTP-to-Module Mapping

| Endpoint | Handler | Core Function |
|----------|---------|---------------|
| POST /tools/search | handle_search | search::search_documents() |
| POST /tools/get | handle_get | get::get_document() |
| GET /tools/sources | handle_sources | sources::get_sources() |
| GET /tools/list | handle_list_tools | tool_script::get_tool_definitions() |
| POST /tools/{name} | handle_tool_call | tool_script::execute_tool() |
| GET /health | handle_health | — |

---

## Stability

If a CLI flag or tool schema changes, this document must be updated.

The public contract is defined by:
- USAGE.md
- SCHEMAS.md
- HYBRID_SCORING.md
- LUA_CONNECTORS.md
- LUA_TOOLS.md
- RUST_TRAITS.md
