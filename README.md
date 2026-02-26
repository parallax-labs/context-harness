# Context Harness

**A local-first context ingestion and retrieval framework for AI tools.**

*by [Parallax Labs](https://github.com/parallax-labs)*

---

Context Harness is a generalized framework for ingesting external knowledge sources into a local, queryable memory store (SQLite + embeddings) and exposing it to developer tools via CLI and MCP-compatible HTTP server.

## Features

- **Connector-driven ingestion** — plug in any source (filesystem, Git repos, S3 buckets, Lua scripts)
- **Extension registries** — install community connectors, tools, and agents from Git-backed repos
- **Local-first storage** — SQLite with FTS5 for keyword search
- **Embedding pipeline** — local (ONNX), Ollama, and OpenAI embeddings with automatic batching, retry, and staleness detection
- **Hybrid retrieval** — keyword + semantic + weighted merge (configurable alpha)
- **MCP server** — expose context to Cursor and other AI tools via HTTP
- **CLI-first** — everything accessible via the `ctx` command
- **Incremental sync** — checkpointed, idempotent, deterministic

## Quick Start

### 1. Install

**Pre-built binaries** (recommended):

Download the latest release for your platform from [GitHub Releases](https://github.com/parallax-labs/context-harness/releases/latest):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-x86_64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-linux-x86_64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# Linux (aarch64)
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-linux-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/
```

Windows: download `ctx-windows-x86_64.zip` from the releases page and add `ctx.exe` to your PATH.

**From source:**

```bash
cargo install --path .
```

### 2. Configure

```bash
cp config/ctx.example.toml config/ctx.toml
# Edit config/ctx.toml with your settings
```

### 3. Initialize

```bash
ctx init --config ./config/ctx.toml
```

### 4. Sync

```bash
# Sync all configured connectors (parallel)
ctx sync all --config ./config/ctx.toml

# Sync all filesystem connectors
ctx sync filesystem --config ./config/ctx.toml

# Sync a specific named instance
ctx sync git:platform --config ./config/ctx.toml

# Sync a Lua script connector
ctx sync script:jira --config ./config/ctx.toml
```

### 5. Search

```bash
# Keyword search (default)
ctx search "your query here" --config ./config/ctx.toml

# Semantic search (requires embedding provider)
ctx search "your query here" --mode semantic --config ./config/ctx.toml

# Hybrid search (keyword + semantic weighted merge)
ctx search "your query here" --mode hybrid --config ./config/ctx.toml
```

### 6. Embeddings

```bash
# Backfill missing embeddings
ctx embed pending --config ./config/ctx.toml

# Rebuild all embeddings
ctx embed rebuild --config ./config/ctx.toml
```

### 7. MCP Server

Start the HTTP server for integration with Cursor, Claude, and other MCP-compatible tools:

```bash
ctx serve mcp --config ./config/ctx.toml
```

The server exposes:
- `/mcp` — MCP Streamable HTTP endpoint (JSON-RPC for Cursor, Claude, etc.)
- `POST /tools/search` — context.search (REST)
- `POST /tools/get` — context.get (REST)
- `GET /tools/sources` — context.sources (REST)
- `GET /health` — health check

**Cursor integration** — start the server, then add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

## Architecture

```
Connectors → Normalization → Chunking → Embedding → SQLite Store → Query Engine → CLI / MCP Server
```

### Data Flow

1. **Connector** pulls items from a source (filesystem, Git, S3, Lua scripts)
2. Items are normalized into a standard `Document`
3. Documents are chunked and stored in SQLite
4. FTS5 index enables keyword search over chunks
5. Chunks are embedded (OpenAI or disabled) and vectors stored as blobs
6. Query engine supports keyword, semantic, and hybrid retrieval
7. Results exposed via CLI and MCP-compatible HTTP server

## CLI Commands

| Command | Description |
|---------|-------------|
| `ctx init` | Initialize database schema |
| `ctx stats` | Show database statistics (docs, chunks, embeddings) |
| `ctx sources` | List available connectors |
| `ctx sync <connector>` | Ingest from a connector (`all`, `git`, `git:name`) |
| `ctx search "<query>"` | Search indexed documents |
| `ctx search --explain` | Search with scoring breakdown per result |
| `ctx get <id>` | Retrieve a document by ID |
| `ctx embed pending` | Backfill missing embeddings |
| `ctx embed rebuild` | Delete and regenerate all embeddings |
| `ctx export` | Export index as JSON for static site search |
| `ctx serve mcp` | Start MCP-compatible HTTP server |
| `ctx connector init <name>` | Scaffold a new Lua connector |
| `ctx connector test <path>` | Test a connector without writing to DB |
| `ctx registry list` | List configured registries and available extensions |
| `ctx registry install` | Clone configured registries |
| `ctx registry update` | Pull latest changes for registries |
| `ctx registry search <q>` | Search extensions by name, tag, or description |
| `ctx registry add <ext>` | Scaffold a config entry for a registry extension |
| `ctx completions <shell>` | Generate shell completions (bash, zsh, fish) |

## HTTP API

The server exposes an MCP Streamable HTTP endpoint and REST endpoints. All REST endpoints return JSON matching the schemas in [`docs/SCHEMAS.md`](docs/SCHEMAS.md).

| Method | Path | Description |
|--------|------|-------------|
| POST | `/mcp` | MCP Streamable HTTP endpoint (JSON-RPC for Cursor, Claude, etc.) |
| POST | `/tools/search` | Search indexed documents (REST) |
| POST | `/tools/get` | Retrieve a document by ID (REST) |
| GET | `/tools/list` | List all registered tools (REST) |
| GET | `/tools/sources` | List connector status (REST) |
| GET | `/agents/list` | List all registered agents (REST) |
| POST | `/agents/{name}/prompt` | Resolve agent prompt (REST) |
| GET | `/health` | Health check |

Errors follow a consistent format:

```json
{
  "error": {
    "code": "not_found",
    "message": "document not found: abc-123"
  }
}
```

## Connector Configuration

All connector types support **named instances** — configure multiple of each:

### Filesystem Connector

```toml
[connectors.filesystem.docs]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt"]
exclude_globs = ["**/drafts/**"]
follow_symlinks = false

[connectors.filesystem.notes]
root = "./notes"
```

### Git Connector

Ingest documentation from any Git repository — point it at a repo URL and subdirectory:

```toml
[connectors.git.platform]
url = "https://github.com/acme/platform.git"   # or git@... or local path
branch = "main"
root = "docs/"                                  # scan this subdirectory
include_globs = ["**/*.md", "**/*.rst"]
shallow = true                                  # --depth 1 clone

[connectors.git.auth-service]
url = "https://github.com/acme/auth-service.git"
branch = "main"
```

Features:
- Clones on first sync, pulls on subsequent syncs
- Per-file last commit timestamp and author from `git log`
- GitHub/GitLab web URLs auto-generated for each file
- Shallow clone support to minimize disk usage
- Incremental sync via checkpoint timestamps

### S3 Connector

Ingest documentation from Amazon S3 buckets:

```toml
[connectors.s3.runbooks]
bucket = "acme-docs"
prefix = "engineering/runbooks/"
region = "us-east-1"
include_globs = ["**/*.md", "**/*.json"]
# endpoint_url = "http://localhost:9000"   # for MinIO / LocalStack
```

Set `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` environment variables.

Features:
- Pagination for large buckets (1000+ objects)
- `LastModified` timestamps for incremental sync
- ETag tracking in metadata
- Custom endpoint URL for S3-compatible services (MinIO, LocalStack)
- Glob-based include/exclude filtering on object keys

### Lua Script Connectors

Write custom connectors in Lua — no recompilation needed. Scripts have access to HTTP, JSON, environment variables, filesystem, base64, crypto, and logging APIs:

```toml
[connectors.script.jira]
path = "connectors/jira.lua"
timeout = 600
url = "https://mycompany.atlassian.net"
api_token = "${JIRA_API_TOKEN}"
project_key = "ENG"
```

```bash
# Scaffold a new connector
ctx connector init jira

# Test it
ctx connector test connectors/jira.lua

# Sync it
ctx sync script:jira
```

See `examples/connectors/github-issues.lua` for a complete example.

## Extension Registries

Install community connectors, tools, and agents from Git-backed repositories. Registries provide ready-to-use Lua extensions that you can install with one command and customize via overrides.

### Install the Community Registry

```bash
ctx registry init --config ./config/ctx.toml
```

Or during first run, `ctx init` will offer to install it automatically.

### Browse and Install Extensions

```bash
# List all available extensions
ctx registry list --config ./config/ctx.toml

# Search for a connector
ctx registry search jira --config ./config/ctx.toml

# See details
ctx registry info connectors/jira --config ./config/ctx.toml

# Add it to your config (scaffolds the TOML entry with placeholders)
ctx registry add connectors/jira --config ./config/ctx.toml
```

Tools and agents from registries are **auto-discovered** at server startup — they appear in `GET /tools/list` and `GET /agents/list` without explicit config. Connectors need credentials, so they require explicit activation via `ctx registry add`.

### Configure Multiple Registries

```toml
[registries.community]
url = "https://github.com/parallax-labs/ctx-registry.git"
path = "~/.ctx/registries/community"
readonly = true
auto_update = true

[registries.company]
url = "git@github.com:myorg/ctx-extensions.git"
path = "~/.ctx/registries/company"
readonly = true
```

Registries are resolved with precedence: explicit config > `.ctx/` project-local > personal > company > community.

### Project-Local Extensions

Place a `.ctx/` directory in your project root with Lua scripts organized as `connectors/<name>/connector.lua`, `tools/<name>/tool.lua`, or `agents/<name>/agent.lua`. They are auto-discovered from any subdirectory.

### Customize an Extension

```bash
# Copy to a writable registry for editing
ctx registry override connectors/jira --config ./config/ctx.toml
```

See [`docs/REGISTRY.md`](docs/REGISTRY.md) for the full specification.

## Embedding Configuration

Context Harness supports three embedding providers:

| Provider | Description | Requires |
|----------|-------------|----------|
| `local` | Built-in ONNX models via fastembed — fully offline | Nothing (model downloads on first use) |
| `ollama` | Local Ollama instance | Running Ollama with an embedding model |
| `openai` | OpenAI API | `OPENAI_API_KEY` env var |

### Local (recommended for offline use)

```toml
[embedding]
provider = "local"
# model = "all-minilm-l6-v2"  # default, 384 dims — no config needed
```

Supported models: `all-minilm-l6-v2` (384d), `bge-small-en-v1.5` (384d), `bge-base-en-v1.5` (768d), `bge-large-en-v1.5` (1024d), `nomic-embed-text-v1` (768d), `nomic-embed-text-v1.5` (768d), `multilingual-e5-small` (384d), `multilingual-e5-base` (768d), `multilingual-e5-large` (1024d).

### Ollama

```toml
[embedding]
provider = "ollama"
model = "nomic-embed-text"
dims = 768
# url = "http://localhost:11434"  # default
```

### OpenAI

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
```

Set the `OPENAI_API_KEY` environment variable before using embedding commands.

## Hybrid Search

Hybrid search merges keyword (FTS5/BM25) and semantic (cosine similarity) signals using a configurable alpha weight:

```toml
[retrieval]
hybrid_alpha = 0.6  # 0.0 = keyword only, 1.0 = semantic only
```

See [`docs/HYBRID_SCORING.md`](docs/HYBRID_SCORING.md) for the full scoring specification.

## Server Configuration

```toml
[server]
bind = "127.0.0.1:7331"
```

## Configuration

See [`config/ctx.example.toml`](config/ctx.example.toml) for a complete example.

## Documentation

Documentation is live at **[parallax-labs.github.io/context-harness](https://parallax-labs.github.io/context-harness/)**.

- **[Docs](https://parallax-labs.github.io/context-harness/docs/)** — getting started, configuration, CLI reference, HTTP API, deployment
- **[API Reference](https://parallax-labs.github.io/context-harness/api/context_harness/)** — Rustdoc API docs generated from source
- **[Live Demo](https://parallax-labs.github.io/context-harness/demo/)** — search a pre-built knowledge base in your browser

### Search Widget

Context Harness ships a drop-in search widget (`ctx-search.js`) for adding ⌘K search to any static site:

```html
<script src="ctx-search.js" data-json="data.json"></script>
```

Build the search index in CI, deploy `data.json` as a static asset, and get instant client-side search — no server, no API keys. See the [docs page](https://parallax-labs.github.io/context-harness/docs/) for a live example.

## License

MIT — see [LICENSE](LICENSE)

## Contributing

Contributions welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
