# Context Harness

[![GitHub stars](https://img.shields.io/github/stars/parallax-labs/context-harness?style=flat)](https://github.com/parallax-labs/context-harness)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/parallax-labs/context-harness?include_prereleases&style=flat)](https://github.com/parallax-labs/context-harness/releases/latest)

**A local-first context ingestion and retrieval framework for AI tools.**

*by [Parallax Labs](https://github.com/parallax-labs)*

**[📖 Documentation & guides](https://parallax-labs.github.io/context-harness/)** · **[Try the demo](https://parallax-labs.github.io/context-harness/demo/)**

---

Context Harness ingests external knowledge (files, Git repos, S3, Lua scripts) into a local SQLite store with optional embeddings, and exposes it via the `ctx` CLI and an MCP-compatible HTTP server so tools like Cursor and Claude can search your context.

## Features

- **Connector-driven ingestion** — plug in any source (filesystem, Git repos, S3 buckets, Lua scripts)
- **Multi-format file support** — plain text (e.g. Markdown, `.txt`) plus **PDF**, **Word** (`.docx`), **PowerPoint** (`.pptx`), and **Excel** (`.xlsx`) with automatic text extraction when you include those extensions in `include_globs`
- **Extension registries** — install community connectors, tools, and agents from Git-backed repos
- **Local-first storage** — SQLite with FTS5 for keyword search
- **Embedding pipeline** — local (fastembed or tract), Ollama, and OpenAI embeddings with automatic batching, retry, and staleness detection
- **Hybrid retrieval** — keyword + semantic + weighted merge (configurable alpha)
- **MCP server** — expose context to Cursor and other AI tools via HTTP
- **CLI-first** — everything accessible via the `ctx` command
- **Incremental sync** — checkpointed, idempotent, deterministic

## Quick Start

For a 5-minute walkthrough with copy-paste config, see the [Quick Start guide](https://parallax-labs.github.io/context-harness/docs/getting-started/quick-start/) on the docs site.

### 1. Install

**Pre-built binaries** (recommended):

Download the latest release from [GitHub Releases](https://github.com/parallax-labs/context-harness/releases/latest):

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

**Nix (NixOS / nix-darwin):**

Install straight from the repo flake — no release tarball needed.

From a clone:

```bash
# Build (full binary with local embeddings)
nix build .#default
./result/bin/ctx --version

# Or install into your user profile (on $PATH)
nix profile install .#default
```

Without cloning (flake URL):

```bash
nix profile install github:parallax-labs/context-harness#default
```

The flake provides two packages:

| Package | Description |
|---------|-------------|
| **`.#default`** | Full build with local embeddings (fastembed; model downloads on first use). |
| **`.#no-local-embeddings`** | Minimal binary, no local embeddings (use OpenAI or Ollama only). |

Use `nix develop` for a development shell. To use Context Harness inside your own flake (NixOS, Home Manager), see the [Nix flake guide](https://parallax-labs.github.io/context-harness/docs/getting-started/nix-flake/).

**From source:**

Local embeddings have **no system dependencies**; models are downloaded on first use.

- **Linux:** Default features use rustls (no system OpenSSL). No extra packages required for a normal `cargo build`.
- **macOS:** The build links against the C++ standard library (used by some dependencies). If you see `library not found for -lc++`, install the Xcode Command Line Tools: `xcode-select --install`. If you use Nix, run `nix develop` first so the shell provides Zig as the C/C++ compiler; then `cargo build` works.

```bash
cargo install --path .
```

### 2. Configure

```bash
cp config/ctx.example.toml config/ctx.toml
# Edit config/ctx.toml with your settings
```

Config path defaults to `./config/ctx.toml`; use `--config` to override. See [configuration reference](https://parallax-labs.github.io/context-harness/docs/reference/configuration/) for all options.

### 3. Initialize and sync

```bash
ctx init
ctx sync all          # sync all connectors in parallel
ctx sync git:platform # or sync a specific connector
```

### 4. Search

```bash
ctx search "your query"              # keyword (default)
ctx search "your query" --mode hybrid   # keyword + semantic (needs embeddings)
ctx embed pending                    # backfill embeddings if using local/ollama/openai
```

### 5. MCP server (Cursor, Claude, etc.)

```bash
ctx serve mcp
```

Add to `.cursor/mcp.json`:

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
5. Chunks are embedded (local, Ollama, or OpenAI) and vectors stored as blobs
6. Query engine supports keyword, semantic, and hybrid retrieval
7. Results exposed via CLI and MCP-compatible HTTP server

## CLI Commands

Full reference: [CLI docs](https://parallax-labs.github.io/context-harness/docs/reference/cli/).

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

The server exposes an MCP Streamable HTTP endpoint and REST endpoints. See [MCP server reference](https://parallax-labs.github.io/context-harness/docs/reference/mcp-server/) for details. REST responses follow the schemas in [`docs/SCHEMAS.md`](docs/SCHEMAS.md).

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

All connector types support **named instances** — configure multiple of each. Full reference: [Built-in connectors](https://parallax-labs.github.io/context-harness/docs/connectors/built-in/).

### Filesystem Connector

Scans a local directory. **Supported formats:** plain text (e.g. `.md`, `.txt`, `.rs`) are always ingested as UTF-8. To also index **PDF**, **Word** (`.docx`), **PowerPoint** (`.pptx`), and **Excel** (`.xlsx`), add those extensions to `include_globs` (e.g. `"**/*.pdf"`, `"**/*.docx"`); they are read as binary and extracted automatically.

```toml
[connectors.filesystem.docs]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt", "**/*.pdf", "**/*.docx"]
exclude_globs = ["**/drafts/**"]
follow_symlinks = false
max_extract_bytes = 50_000_000 # skip files larger than this (default: 50MB)

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

Install community connectors, tools, and agents from Git-backed repositories. See [Registry overview](https://parallax-labs.github.io/context-harness/docs/registry/overview/) and [Usage guide](https://parallax-labs.github.io/context-harness/docs/registry/usage-guide/) on the docs site.

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

See the [registry docs](https://parallax-labs.github.io/context-harness/docs/registry/overview/) for the full specification.

## Embedding Configuration

Context Harness supports three embedding providers:

| Provider | Description | Requires |
|----------|-------------|----------|
| `local` | Built-in models via fastembed (primary) or tract (musl/Intel Mac) — fully offline | No system deps; model downloads on first use |
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

### Platform support (release binaries)

Pre-built [release binaries](https://github.com/parallax-labs/context-harness/releases) are built for six targets. The **local** embedding provider is included on all targets: primary platforms use fastembed (bundled ORT); Linux musl and macOS Intel use a pure-Rust (tract) backend.

| Binary | Local embeddings | OpenAI / Ollama |
|--------|------------------|------------------|
| Linux x86_64 (glibc) | ✅ fastembed | ✅ |
| Linux x86_64 (musl) | ✅ tract | ✅ |
| Linux aarch64 | ✅ fastembed | ✅ |
| macOS x86_64 (Intel) | ✅ tract | ✅ |
| macOS aarch64 (Apple Silicon) | ✅ fastembed | ✅ |
| Windows x86_64 | ✅ fastembed | ✅ |

- **Minimal binary (no local embeddings):** `cargo install --path . --no-default-features`
- **From source on musl or Intel Mac (tract backend):** `cargo build --no-default-features --features local-embeddings-tract`
- **CI/release:** Linux cross-builds (musl, aarch64) use Zig and [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild); no cross-rs or system OpenSSL.

See the [configuration docs](https://parallax-labs.github.io/context-harness/docs/reference/configuration/) for full platform notes.

## Hybrid Search

Hybrid search merges keyword (FTS5/BM25) and semantic (cosine similarity) signals with a configurable alpha:

```toml
[retrieval]
hybrid_alpha = 0.6  # 0.0 = keyword only, 1.0 = semantic only
```

Use `ctx search "query" --mode hybrid --explain` to see score breakdowns. See [search reference](https://parallax-labs.github.io/context-harness/docs/reference/search/) and [`docs/HYBRID_SCORING.md`](docs/HYBRID_SCORING.md) for the full specification.

## Server Configuration

```toml
[server]
bind = "127.0.0.1:7331"
```

For production (Docker, systemd, CI), see [Deployment](https://parallax-labs.github.io/context-harness/docs/reference/deployment/).

## Configuration

See [`config/ctx.example.toml`](config/ctx.example.toml) for a complete example, or the [configuration reference](https://parallax-labs.github.io/context-harness/docs/reference/configuration/) on the docs site.

## Documentation

**Website: [parallax-labs.github.io/context-harness](https://parallax-labs.github.io/context-harness/)**

| Link | Description |
|------|-------------|
| [Getting started](https://parallax-labs.github.io/context-harness/docs/getting-started/quick-start/) | Quick Start, [Installation](https://parallax-labs.github.io/context-harness/docs/getting-started/installation/), [Nix flake](https://parallax-labs.github.io/context-harness/docs/getting-started/nix-flake/) |
| [Configuration](https://parallax-labs.github.io/context-harness/docs/reference/configuration/) | Full `ctx.toml` reference, embedding providers, platform table |
| [CLI reference](https://parallax-labs.github.io/context-harness/docs/reference/cli/) | Every command and flag |
| [Connectors & registry](https://parallax-labs.github.io/context-harness/docs/connectors/built-in/) | Built-in connectors, [Lua connectors](https://parallax-labs.github.io/context-harness/docs/connectors/lua-connectors/), [extension registry](https://parallax-labs.github.io/context-harness/docs/registry/overview/) |
| [Guides](https://parallax-labs.github.io/context-harness/docs/guides/agents/) | Agent integration, Cursor, RAG, multi-repo, deployment |
| [API (Rustdoc)](https://parallax-labs.github.io/context-harness/api/context_harness/) | Generated Rust API docs |
| [Live demo](https://parallax-labs.github.io/context-harness/demo/) | Search a pre-built knowledge base in the browser |

The site also documents the **search widget** (`ctx-search.js`) for adding ⌘K search to static sites — see the [docs](https://parallax-labs.github.io/context-harness/docs/) for an example.

If Context Harness is useful to you, consider [starring the repo](https://github.com/parallax-labs/context-harness).

## License

AGPL-3.0 — see [LICENSE](LICENSE)

For commercial licensing inquiries, contact [Parallax Labs](https://github.com/parallax-labs).

## Contributing

Contributions welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
