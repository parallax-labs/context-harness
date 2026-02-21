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

Guarantees:
- Document model exists
- Chunk model exists
- normalize(SourceItem) -> Document
- chunk(Document) -> Vec<Chunk>

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

Database tables guaranteed:
- documents
- chunks
- checkpoints
- chunks_fts (FTS5)

---

### retrieval/ (src/)

- `search.rs` — Keyword search with BM25 scoring

Guarantees:
- Keyword search (FTS5)
- Document-level grouping
- Score normalization to [0, 1]
- Deterministic tie-breaking

---

### interfaces/ (src/)

- `main.rs` — CLI with clap
- `get.rs` — Document retrieval
- `sources.rs` — Connector listing

---

## Data Flow Guarantee

sync command must:

```
Connector -> SourceItem
SourceItem -> normalize() -> Document
Document -> upsert_document()
Document -> chunk()
Chunks -> replace_chunks()
Chunks -> embed() if enabled
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

---

## Stability

If a CLI flag or tool schema changes, this document must be updated.

The public contract is defined by:
- USAGE.md
- SCHEMAS.md

