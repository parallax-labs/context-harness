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
- Exposes retrieval via CLI and MCP-compatible HTTP server

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

### Connector Config

```toml
# Filesystem — scan local directory
[connectors.filesystem]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt"]
exclude_globs = []
follow_symlinks = false

# Git — clone and scan a Git repository
[connectors.git]
url = "https://github.com/acme/platform.git"
branch = "main"
root = "docs/"
include_globs = ["**/*.md"]
shallow = true
# cache_dir = "./data/.git-cache/platform"  # optional

# S3 — scan an Amazon S3 bucket
# Requires: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY
[connectors.s3]
bucket = "acme-docs"
prefix = "engineering/runbooks/"
region = "us-east-1"
include_globs = ["**/*.md"]
# endpoint_url = "http://localhost:9000"     # for MinIO
```

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
ctx sync <connector>  # connector: filesystem, git, s3
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

### 8. serve mcp

Start the MCP-compatible HTTP tool server.

```bash
ctx serve mcp
```

Required behavior:
- Bind to `[server].bind` address
- Expose endpoints per SCHEMAS.md:
  - `POST /tools/search` — context.search
  - `POST /tools/get` — context.get
  - `GET /tools/sources` — context.sources
  - `GET /health` — health check
- All responses must match SCHEMAS.md exactly
- All errors must follow error schema
- CORS enabled for cross-origin requests

---

## HTTP Endpoints

See [SCHEMAS.md](SCHEMAS.md) for complete request/response schemas.

| Method | Path | Tool | Description |
|--------|------|------|-------------|
| POST | /tools/search | context.search | Search indexed documents |
| POST | /tools/get | context.get | Retrieve a document by ID |
| GET | /tools/sources | context.sources | List connector status |
| GET | /health | — | Health check |

---

## Stability Guarantee

The commands above and their flags are considered the stable public interface.
Implementation must conform to this contract.
