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
provider = "disabled"       # "disabled" | "openai" | "ollama" | "local"
# model = "text-embedding-3-small"
# dims = 1536
# batch_size = 64
# max_retries = 5
# timeout_secs = 30
# url = "http://localhost:11434"   # Ollama API base URL (ollama only)

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
When `embedding.provider` is `openai` or `ollama`, `model` and `dims` are required.
When `embedding.provider` is `local`, model and dims are optional (defaults to `all-minilm-l6-v2`, 384 dims).

### Connector Config

```toml
# Filesystem — scan local directory (named instance)
[connectors.filesystem.docs]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt"]
exclude_globs = []
follow_symlinks = false

# Git — clone and scan a Git repository (named instance)
[connectors.git.platform]
url = "https://github.com/acme/platform.git"
branch = "main"
root = "docs/"
include_globs = ["**/*.md"]
shallow = true
# cache_dir = "./data/.git-cache/platform"  # optional

# S3 — scan an Amazon S3 bucket (named instance)
# Requires: AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY
[connectors.s3.runbooks]
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
- `--explain` — show scoring breakdown per result

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

### 8. stats

Show database statistics — document, chunk, and embedding counts with per-source breakdown.

```bash
ctx stats
```

Required behavior:
- Show total document, chunk, and embedding counts
- Show embedding coverage percentage
- Show per-source breakdown with last sync timestamp
- Show database file size

---

### 9. search --explain

The `search` command accepts an optional `--explain` flag that shows the scoring breakdown for each result.

```bash
ctx search "<query>" --explain
```

Required flags (in addition to existing flags):
- `--explain` — show keyword_score, semantic_score, hybrid_score, alpha, and candidate pool sizes

---

### 10. export

Export the search index as JSON for static site search with `ctx-search.js`.

```bash
ctx export
ctx export --output data.json
```

Required flags:
- `--output <path>` — output file path (defaults to stdout)

Required behavior:
- Export all documents and chunks as JSON
- Match the schema expected by `ctx-search.js`
- Print summary to stderr when writing to file

---

### 11. completions

Generate shell completion scripts.

```bash
ctx completions bash > ~/.local/share/bash-completion/completions/ctx
ctx completions zsh > ~/.zfunc/_ctx
ctx completions fish > ~/.config/fish/completions/ctx.fish
```

Required behavior:
- Accept shell name: `bash`, `zsh`, `fish`, `elvish`, `powershell`
- Print completion script to stdout

---

### 12. serve mcp

Start the MCP-compatible HTTP tool server.

```bash
ctx serve mcp
```

Required behavior:
- Bind to `[server].bind` address
- Expose MCP Streamable HTTP endpoint at `/mcp` (JSON-RPC for Cursor, Claude, etc.)
- Expose REST endpoints per SCHEMAS.md:
  - `POST /tools/search` — context.search
  - `POST /tools/get` — context.get
  - `GET /tools/sources` — context.sources
  - `GET /health` — health check
- All REST responses must match SCHEMAS.md exactly
- All errors must follow error schema
- CORS enabled for cross-origin requests

---

### 13. registry

Manage extension registries — community connectors, tools, and agents hosted in Git repositories.

```bash
ctx registry list                         # show registries and extensions
ctx registry install [name]               # clone configured registries
ctx registry update [name]                # git pull registries
ctx registry search <query>               # search extensions by name/tag/description
ctx registry info <type/name>             # show extension details
ctx registry add <type/name>              # scaffold config entry in ctx.toml
ctx registry override <type/name>         # copy extension to writable registry
ctx registry init                         # install the community registry
```

Required behavior:
- `list` — load all configured registries, print extensions grouped by type with source registry
- `install` — shallow clone git-backed registries that aren't yet present on disk
- `update` — `git pull --ff-only` on git-backed registries; skip dirty working trees
- `search` — case-insensitive match against extension name, description, and tags
- `info` — print description, tags, required config, and README if present
- `add` — append a `[connectors.script.<name>]`, `[tools.script.<name>]`, or `[agents.script.<name>]` section to the config file using `config.example.toml` from the extension directory
- `override` — copy extension directory from a readonly registry to the first writable registry
- `init` — clone the community registry and add `[registries.community]` to config

### Registry Configuration

```toml
[registries.community]
url = "https://github.com/parallax-labs/ctx-registry.git"
branch = "main"
path = "~/.ctx/registries/community"
readonly = true
auto_update = true

[registries.company]
url = "git@github.com:myorg/ctx-extensions.git"
branch = "main"
path = "~/.ctx/registries/company"
readonly = true
auto_update = true

[registries.personal]
path = "~/.ctx/extensions"
readonly = false
```

Registry fields:
- `url` — Git repository URL (optional; omit for local-only registries)
- `branch` — Git branch or tag to track (default: `"main"`)
- `path` — Local filesystem path where the registry is stored
- `readonly` — If `true`, extensions cannot be edited in place
- `auto_update` — If `true`, registries are pulled during updates

### `.ctx/` Project-Local Extensions

A `.ctx/` directory in the current working directory (or any ancestor) is auto-discovered as a project-local registry with the highest precedence. No config entry needed.

### Extension Auto-Discovery

Tools and agents from registries are **auto-discovered** at server startup — they appear in `GET /tools/list` and `GET /agents/list` without explicit config entries.

Connectors from registries require explicit activation via `ctx registry add` because they need credentials.

---

## HTTP Endpoints

See [SCHEMAS.md](SCHEMAS.md) for complete request/response schemas.

| Method | Path | Description |
|--------|------|-------------|
| POST | /mcp | MCP Streamable HTTP endpoint (JSON-RPC for Cursor, Claude, etc.) |
| POST | /tools/search | Search indexed documents (REST) |
| POST | /tools/get | Retrieve a document by ID (REST) |
| GET | /tools/sources | List connector status (REST) |
| GET | /tools/list | List all registered tools (REST) |
| GET | /agents/list | List all registered agents (REST) |
| POST | /agents/{name}/prompt | Resolve agent prompt (REST) |
| GET | /health | Health check |

---

## Stability Guarantee

The commands above and their flags are considered the stable public interface.
Implementation must conform to this contract.
