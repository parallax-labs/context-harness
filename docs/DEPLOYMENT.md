# Context Harness — Deployment Guide

This guide covers every deployment scenario for Context Harness: local development, production servers, CI/CD pipelines, browser-only demos, and Cursor/MCP integration.

---

## Table of Contents

- [Prerequisites](#prerequisites)
- [Building from Source](#building-from-source)
- [Local Development](#local-development)
- [Production Deployment](#production-deployment)
  - [Single Binary](#single-binary)
  - [Systemd Service](#systemd-service)
  - [Docker](#docker)
- [CI/CD Pipeline](#cicd-pipeline)
  - [GitHub Actions CI](#github-actions-ci)
  - [Documentation Site Deployment](#documentation-site-deployment)
- [Cursor / MCP Integration](#cursor--mcp-integration)
- [Browser-Only Demo](#browser-only-demo)
- [Documentation Site (Dogfooding)](#documentation-site-dogfooding)
- [Environment Variables](#environment-variables)
- [Configuration Reference](#configuration-reference)
- [Troubleshooting](#troubleshooting)

---

## Prerequisites

| Tool | Version | Required For |
|------|---------|-------------|
| **Rust** | 1.75+ (stable) | Building the `ctx` binary |
| **Git** | 2.x | Git connector, cloning repos |
| **Python 3** | 3.8+ | `build-docs.sh` (JSON export) |
| **SQLite** | 3.35+ | Bundled via `sqlx`; no system install needed |

Optional:

| Tool | Required For |
|------|-------------|
| `OPENAI_API_KEY` | OpenAI embeddings (semantic/hybrid search) |
| `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` | S3 connector |
| Docker | Containerized deployment |

---

## Building from Source

```bash
# Clone the repository
git clone https://github.com/parallax-labs/context-harness.git
cd context-harness

# Build in release mode (recommended for production)
cargo build --release

# The binary is at ./target/release/ctx
./target/release/ctx --help
```

### Install Globally

```bash
# Install to ~/.cargo/bin/ctx
cargo install --path .

# Or install directly from the repo
cargo install --git https://github.com/parallax-labs/context-harness
```

### Verify Installation

```bash
ctx --version
# context-harness 0.1.0
```

---

## Local Development

### 1. Copy the Example Config

```bash
cp config/ctx.example.toml config/ctx.toml
```

### 2. Edit Configuration

Point `[connectors.filesystem.<name>].root` at your docs directory:

```toml
[connectors.filesystem.docs]
root = "./my-project/docs"
include_globs = ["**/*.md", "**/*.txt", "**/*.rs"]
```

### 3. Initialize and Sync

```bash
# Create the database
ctx init --config ./config/ctx.toml

# Ingest documents
ctx sync filesystem --config ./config/ctx.toml

# Verify
ctx sources --config ./config/ctx.toml
ctx search "hello world" --config ./config/ctx.toml
```

### 4. Enable Embeddings (Optional)

```bash
export OPENAI_API_KEY="sk-..."
```

Update your config:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
```

Then generate embeddings:

```bash
ctx embed pending --config ./config/ctx.toml
ctx search "deployment" --mode hybrid --config ./config/ctx.toml
```

### 5. Run Tests

```bash
# Unit + integration tests
cargo test

# With formatting and lint checks
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

### 6. Generate Rustdoc

```bash
cargo doc --no-deps --document-private-items --open
```

---

## Production Deployment

### Single Binary

Context Harness compiles to a single static binary with no runtime dependencies (SQLite is embedded). Deploy by copying the binary and a config file:

```bash
# Build
cargo build --release

# Deploy
scp target/release/ctx server:/opt/context-harness/ctx
scp config/ctx.toml server:/opt/context-harness/config/ctx.toml

# On the server
/opt/context-harness/ctx init --config /opt/context-harness/config/ctx.toml
/opt/context-harness/ctx sync filesystem --config /opt/context-harness/config/ctx.toml
/opt/context-harness/ctx serve mcp --config /opt/context-harness/config/ctx.toml
```

### Systemd Service

Create `/etc/systemd/system/context-harness.service`:

```ini
[Unit]
Description=Context Harness MCP Server
After=network.target

[Service]
Type=simple
ExecStart=/opt/context-harness/ctx serve mcp --config /opt/context-harness/config/ctx.toml
Environment=OPENAI_API_KEY=sk-...
Restart=on-failure
RestartSec=5
User=ctx
Group=ctx
WorkingDirectory=/opt/context-harness

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable context-harness
sudo systemctl start context-harness
sudo systemctl status context-harness
```

### Cron-Based Sync

Add a cron job for periodic re-ingestion:

```bash
# Sync every 15 minutes
*/15 * * * * /opt/context-harness/ctx sync filesystem --config /opt/context-harness/config/ctx.toml >> /var/log/ctx-sync.log 2>&1
```

### Docker

Create a `Dockerfile`:

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ctx /usr/local/bin/ctx

WORKDIR /data
COPY config/ctx.example.toml /etc/ctx/ctx.toml

EXPOSE 7331

ENTRYPOINT ["ctx"]
CMD ["serve", "mcp", "--config", "/etc/ctx/ctx.toml"]
```

Build and run:

```bash
docker build -t context-harness .

# Initialize
docker run --rm -v ctx-data:/data context-harness init --config /etc/ctx/ctx.toml

# Sync
docker run --rm -v ctx-data:/data context-harness sync filesystem --config /etc/ctx/ctx.toml

# Serve
docker run -d --name ctx-server -p 7331:7331 -v ctx-data:/data context-harness
```

---

## CI/CD Pipeline

### GitHub Actions CI

The project includes a CI workflow at `.github/workflows/ci.yml` that runs on every push and PR to `main`:

```yaml
# What CI checks:
- cargo fmt --all -- --check     # formatting
- cargo clippy -- -D warnings    # linting
- cargo test                     # unit + integration tests
```

### Documentation Site Deployment

The documentation site is automatically built and deployed to GitHub Pages via `.github/workflows/pages.yml`:

**What the pipeline does:**

1. Checks out the repository
2. Builds the `ctx` binary in release mode
3. Runs `scripts/build-docs.sh` which:
   - Generates rustdoc API reference
   - Uses the **Git connector** to ingest the project's own docs and source files
   - Exports the indexed data as JSON for the browser-based search page
   - Optionally generates embeddings if `OPENAI_API_KEY` is available as a secret
4. Uploads the `site/` directory as a GitHub Pages artifact
5. Deploys to GitHub Pages

**Required GitHub configuration:**

1. Enable GitHub Pages in Settings → Pages → Source: "GitHub Actions"
2. (Optional) Add `OPENAI_API_KEY` as a repository secret for semantic search

**Triggers:**

- Push to `main` that modifies `site/`, `src/`, `docs/`, `scripts/`, `Cargo.toml`, or `Cargo.lock`
- Manual trigger via `workflow_dispatch`

### Building Documentation Locally

```bash
# Build everything (binary, rustdoc, search data)
./scripts/build-docs.sh

# Serve locally to verify
cd site && python3 -m http.server 8080
# Open http://localhost:8080
```

---

## Cursor / MCP Integration

Context Harness can serve as a context provider for Cursor, Claude, and other MCP-compatible AI tools.

### Setup

1. **Build and install** the `ctx` binary:

```bash
cargo install --path .
```

2. **Initialize and sync** your project's documentation:

```bash
ctx init --config /path/to/ctx.toml
ctx sync filesystem --config /path/to/ctx.toml
```

3. **Start the MCP server:**

```bash
ctx serve mcp --config /path/to/ctx.toml
```

4. **Add to Cursor's MCP configuration** (`.cursor/mcp.json` or global settings):

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

5. **Verify** — Cursor connects to the `/mcp` Streamable HTTP endpoint and shows it in the MCP panel. The agent can now use:
   - `context.search` — search your indexed documents
   - `context.get` — retrieve full documents by ID
   - `context.sources` — check connector status

### Multi-Project Setup

You can run multiple Context Harness instances for different projects by creating separate config files with unique database paths and server ports:

```bash
# Terminal 1: project alpha on port 7331
ctx serve mcp --config /projects/alpha/ctx.toml

# Terminal 2: project beta on port 7332
ctx serve mcp --config /projects/beta/ctx.toml
```

```json
{
  "mcpServers": {
    "project-alpha": {
      "url": "http://127.0.0.1:7331/mcp"
    },
    "project-beta": {
      "url": "http://127.0.0.1:7332/mcp"
    }
  }
}
```

### Recommended Configuration for Cursor

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 500        # Smaller chunks = more precise retrieval

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536

[retrieval]
hybrid_alpha = 0.6      # Balance keyword + semantic
final_limit = 8         # Don't overwhelm the context window

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem.local]
root = "./"
include_globs = ["**/*.md", "**/*.rs", "**/*.ts", "**/*.py"]
exclude_globs = ["**/target/**", "**/node_modules/**", "**/.git/**"]
```

---

## Browser-Only Demo

The browser-only demo at `site/demo/` runs entirely client-side with no server required. It uses:

- **sql.js** — SQLite compiled to WebAssembly
- **Transformers.js** — embedding models running in the browser via WASM
- **BM25 in JavaScript** — pure JS keyword search

### How It Works

1. At build time, documents are ingested and exported as `data.json`
2. The browser loads `data.json`, initializes an in-memory SQLite database, and indexes all chunks
3. Search runs entirely in the browser — no network requests needed

### Deploying the Demo

The demo is deployed automatically as part of the GitHub Pages workflow. To deploy manually:

```bash
# Build the demo data
./scripts/build-docs.sh

# Deploy site/ to any static hosting
# (Netlify, Vercel, S3, GitHub Pages, etc.)
```

---

## Documentation Site (Dogfooding)

The searchable documentation at `site/docs/` is a live example of Context Harness dogfooding itself:

1. The **Git connector** indexes this repo's `docs/`, `src/`, `README.md`, `CHANGELOG.md`, and `CONTRIBUTING.md`
2. Documents are chunked and (optionally) embedded
3. The search data is exported as `data.json`
4. The browser-based search page provides full-text and semantic search
5. A **Chat tab** allows conversational Q&A over the docs using WebLLM (offline) or OpenAI

### Adding New Documentation

1. Create or edit files in `docs/` or `src/`
2. Push to `main` — the CI pipeline will automatically:
   - Re-index the docs via the Git connector
   - Re-generate rustdoc
   - Re-deploy the site

---

## Environment Variables

| Variable | Required For | Description |
|----------|-------------|-------------|
| `OPENAI_API_KEY` | Embeddings | OpenAI API key for `text-embedding-3-small` |
| `AWS_ACCESS_KEY_ID` | S3 connector | AWS access key |
| `AWS_SECRET_ACCESS_KEY` | S3 connector | AWS secret key |
| `AWS_SESSION_TOKEN` | S3 connector | AWS session token (optional, for temporary credentials) |

---

## Configuration Reference

See [`config/ctx.example.toml`](../config/ctx.example.toml) for a complete annotated example.

### Key Sections

| Section | Purpose |
|---------|---------|
| `[db]` | SQLite database file path |
| `[chunking]` | Token limits for text chunking |
| `[embedding]` | Provider, model, dimensions, batching |
| `[retrieval]` | Hybrid alpha, candidate counts, result limits |
| `[server]` | HTTP bind address |
| `[connectors.filesystem.<name>]` | Named local directory scanning |
| `[connectors.git.<name>]` | Named Git repository ingestion |
| `[connectors.s3.<name>]` | Named S3 bucket scanning |
| `[connectors.script.<name>]` | Named Lua scripted connectors |

### Hybrid Alpha Tuning

The `hybrid_alpha` parameter controls the balance between keyword and semantic search:

| Value | Behavior |
|-------|----------|
| `0.0` | Pure keyword search (FTS5/BM25) |
| `0.3` | Keyword-heavy hybrid |
| `0.6` | Balanced hybrid (default) |
| `0.8` | Semantic-heavy hybrid |
| `1.0` | Pure semantic search (cosine similarity) |

---

## Troubleshooting

### Database Locked

If you see "database is locked" errors, ensure only one `ctx sync` process runs at a time. The SQLite WAL mode supports concurrent reads but only one writer.

### Git Connector: Authentication

For private repositories, ensure your Git credentials are configured:

```bash
# HTTPS: use a personal access token
git config --global credential.helper store

# SSH: ensure your SSH key is loaded
ssh-add ~/.ssh/id_ed25519
```

### S3 Connector: Permissions

The IAM user/role needs `s3:ListBucket` and `s3:GetObject` permissions on the target bucket and prefix.

### Embedding Failures

Embedding is non-fatal during sync — if the OpenAI API is unavailable, documents are still ingested. Run `ctx embed pending` later to backfill:

```bash
ctx embed pending --config ./config/ctx.toml
```

### Large Repositories

For large Git repositories, use shallow clones to minimize disk usage:

```toml
[connectors.git.platform]
shallow = true        # --depth 1 clone
root = "docs/"        # only scan a subdirectory
```

### Port Conflicts

If the MCP server fails to bind, change the port in config:

```toml
[server]
bind = "127.0.0.1:8080"
```

### Rebuilding Embeddings

If you change the embedding model or dimensions, rebuild all embeddings:

```bash
ctx embed rebuild --config ./config/ctx.toml
```

