# Context Harness

**A local-first context ingestion and retrieval framework for AI tools.**

*by [Parallax Labs](https://github.com/parallax-labs)*

---

Context Harness is a generalized framework for ingesting external knowledge sources into a local, queryable memory store (SQLite + embeddings) and exposing it to developer tools via CLI and MCP-compatible HTTP server.

## Features

- **Connector-driven ingestion** — plug in any source (filesystem, GitHub, Jira, Slack)
- **Local-first storage** — SQLite with FTS5 for keyword search
- **Embedding pipeline** — OpenAI embeddings with automatic batching, retry, and staleness detection
- **Hybrid retrieval** — keyword + semantic + weighted merge (configurable alpha)
- **MCP server** — expose context to Cursor and other AI tools via HTTP
- **CLI-first** — everything accessible via the `ctx` command
- **Incremental sync** — checkpointed, idempotent, deterministic

## Quick Start

### 1. Install

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
ctx sync filesystem --config ./config/ctx.toml
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
- `POST /tools/search` — context.search
- `POST /tools/get` — context.get
- `GET /tools/sources` — context.sources
- `GET /health` — health check

**Cursor integration** — add to your MCP config:

```json
{
  "mcpServers": {
    "context-harness": {
      "command": "ctx",
      "args": ["--config", "/path/to/ctx.toml", "serve", "mcp"]
    }
  }
}
```

## Architecture

```
Connectors → Normalization → Chunking → Embedding → SQLite Store → Query Engine → CLI / MCP Server
```

### Data Flow

1. **Connector** pulls items from a source
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
| `ctx sources` | List available connectors |
| `ctx sync <connector>` | Ingest data from a connector |
| `ctx search "<query>"` | Search indexed documents |
| `ctx get <id>` | Retrieve a document by ID |
| `ctx embed pending` | Backfill missing embeddings |
| `ctx embed rebuild` | Delete and regenerate all embeddings |
| `ctx serve mcp` | Start MCP-compatible HTTP server |

## HTTP API

All endpoints return JSON matching the schemas in [`docs/SCHEMAS.md`](docs/SCHEMAS.md).

| Method | Path | Description |
|--------|------|-------------|
| POST | `/tools/search` | Search indexed documents |
| POST | `/tools/get` | Retrieve a document by ID |
| GET | `/tools/sources` | List connector status |
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

## Embedding Configuration

To enable embeddings, set the `[embedding]` section in your config:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
max_retries = 5
timeout_secs = 30
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

## License

MIT — see [LICENSE](LICENSE)

## Contributing

Contributions welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
