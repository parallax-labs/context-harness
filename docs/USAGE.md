# Context Harness — Usage Contract

This document defines the public surface area of Context Harness.
Anything described here MUST be supported by the implementation.

---

## Overview

Context Harness is a local-first context indexing framework that:

- Ingests data from external systems (connectors)
- Normalizes into a Document model
- Chunks and embeds content
- Stores in SQLite
- Exposes retrieval via CLI and MCP-compatible server

---

## Configuration

Context Harness MUST accept a `--config` flag pointing to a TOML file.

```bash
ctx --config ./config/ctx.toml <command>
```

### Required Config Fields

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "disabled"       # "disabled" | "openai"
# model = "text-embedding-3-small"
# dims = 1536
# batch_size = 64
# max_retries = 5
# timeout_secs = 30

[retrieval]
final_limit = 12
hybrid_alpha = 0.6
candidate_k_keyword = 80
candidate_k_vector = 80
group_by = "document"
doc_agg = "max"
max_chunks_per_doc = 3

[server]
bind = "127.0.0.1:7331"
```

All fields above MUST exist in config schema.
When `embedding.provider != "disabled"`, `model` and `dims` are required.

---

## CLI Commands

### 1. init

Initializes database schema.

```bash
ctx init
```

Required behavior:
- Create SQLite database if missing
- Create required tables (documents, chunks, checkpoints, chunks_fts, embeddings, chunk_vectors)
- Create FTS index
- Print success message
- Must be idempotent

---

### 2. sources

Lists available connectors and their status.

```bash
ctx sources
```

Output:

```
filesystem  OK
slack       NOT CONFIGURED
github      NOT CONFIGURED
```

---

### 3. sync

Ingest from connector.

```bash
ctx sync <connector>
```

Required flags:
- `--full`
- `--dry-run`
- `--since <date>`
- `--until <date>`
- `--limit <n>`

Required behavior:
- Use checkpoint unless `--full`
- Upsert documents
- Chunk documents
- Embed inline if provider enabled (non-fatal on failure)
- Update checkpoint
- Print summary stats (including embeddings_written, embeddings_pending if enabled)

---

### 4. search

Hybrid retrieval (default mode: keyword).

```bash
ctx search "<query>"
```

Required flags:
- `--mode keyword|semantic|hybrid`
- `--source <name>`
- `--since <date>`
- `--limit <n>`

Required behavior:
- Return ranked results
- Show score (normalized to [0,1])
- Show snippet
- Show document id
- `--mode keyword`: FTS5 only
- `--mode semantic`: vector only (requires embeddings enabled)
- `--mode hybrid`: weighted merge per HYBRID_SCORING.md
- Error cleanly if semantic/hybrid requested with embeddings disabled

---

### 5. get

Retrieve full document.

```bash
ctx get <id>
```

Required behavior:
- Return metadata
- Return body
- Return chunks
- Handle missing ID with error + nonzero exit

---

### 6. embed pending

Backfill missing or stale embeddings.

```bash
ctx embed pending
```

Required flags:
- `--limit <n>`
- `--batch-size <n>` (overrides config)
- `--dry-run`

Required behavior:
- Find chunks without embeddings for current model, or with stale hash
- Embed in batches
- Upsert embedding metadata + vector
- Print summary (total, embedded, failed)
- Error if provider disabled

---

### 7. embed rebuild

Delete and regenerate all embeddings.

```bash
ctx embed rebuild
```

Required flags:
- `--batch-size <n>` (overrides config)

Required behavior:
- Delete all existing embeddings (metadata + vectors)
- Re-embed all chunks for the configured model
- Print summary (total, embedded, failed)
- Error if provider disabled

---

## Stability Guarantee

The commands above and their flags are considered the stable public interface.
Implementation must conform to this contract.
