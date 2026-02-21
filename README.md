# Context Harness

**A local-first context ingestion and retrieval framework for AI tools.**

*by [Parallax Labs](https://github.com/parallax-labs)*

---

Context Harness is a generalized framework for ingesting external knowledge sources into a local, queryable memory store (SQLite + embeddings) and exposing it to developer tools via CLI and MCP server.

## Features

- **Connector-driven ingestion** — plug in any source (filesystem, GitHub, Jira, Slack)
- **Local-first storage** — SQLite with FTS5 for keyword search
- **Hybrid retrieval** — keyword + semantic + weighted merge
- **MCP server** — expose context to Cursor and other AI tools
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
ctx search "your query here" --config ./config/ctx.toml
```

## Architecture

```
Connectors → Normalization → Chunking → SQLite Store → Query Engine → CLI / MCP
```

### Data Flow

1. **Connector** pulls items from a source
2. Items are normalized into a standard `Document`
3. Documents are chunked and stored in SQLite
4. FTS5 index enables keyword search over chunks
5. Query engine supports keyword, semantic, and hybrid retrieval
6. Results exposed via CLI and MCP server

## CLI Commands

| Command | Description |
|---------|-------------|
| `ctx init` | Initialize database schema |
| `ctx sources` | List available connectors |
| `ctx sync <connector>` | Ingest data from a connector |
| `ctx search "<query>"` | Search indexed documents |
| `ctx get <id>` | Retrieve a document by ID |

## Configuration

See [`config/ctx.example.toml`](config/ctx.example.toml) for a complete example.

## License

MIT — see [LICENSE](LICENSE)

## Contributing

Contributions welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

