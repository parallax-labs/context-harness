# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Lua MCP tool extensions** — define custom MCP tools in Lua that AI agents can discover via `GET /tools/list` and call via `POST /tools/{name}`. Tool scripts define a `tool` table with `name`, `description`, `parameters`, and an `execute(params, context)` function. The `context` bridge provides `search()`, `get()`, `sources()`, and `config` for RAG-powered tools. Parameter schemas are converted to OpenAI function-calling JSON Schema format.
- **`GET /tools/list`** endpoint — returns all registered tools (built-in + Lua) with their parameter schemas.
- **`POST /tools/{name}`** endpoint — calls a registered Lua tool with validated parameters. Returns `400` for validation errors, `404` for unknown tools, `408` for timeouts, `500` for script errors.
- **`ctx tool init <name>`** — scaffold a new tool script from a template.
- **`ctx tool test <path>`** — test a tool script with `--param key=value` pairs.
- **`ctx tool list`** — list all configured tools (built-in and Lua) with descriptions.
- **`[tools.script.<name>]`** config section — TOML config with `path`, `timeout`, and arbitrary keys accessible as `context.config` in the script. Values support `${VAR_NAME}` environment variable expansion.
- **`lua_runtime.rs`** — extracted shared Lua VM setup, sandboxing, and host APIs into a reusable module used by both connectors and tools.
- Example echo tool (`examples/tools/echo.lua`) — minimal test tool demonstrating parameter handling and context bridge.
- Example Jira tool (`examples/tools/create-jira-ticket.lua`) — full example with RAG enrichment and HTTP API calls.
- Test fixture tool (`tests/fixtures/test_tool.lua`).
- **Lua scripted connectors** — extend Context Harness with custom data sources by writing Lua scripts instead of compiling Rust. Scripts implement `connector.scan(config) → items[]` and have access to sandboxed host APIs: `http` (GET/POST/PUT), `json` (parse/encode), `env` (read env vars), `log` (info/warn/error/debug), `fs` (sandboxed read/list), `base64` (encode/decode), `crypto` (sha256/hmac_sha256), and `sleep`. Scripts run in a sandboxed Lua 5.4 VM with timeout protection.
- **`ctx connector init <name>`** — scaffold a new connector from a template.
- **`ctx connector test <path>`** — test a connector script without writing to the database.
- **`ctx sync script:<name>`** — sync a specific Lua connector; `ctx sync script` syncs all.
- **`[connectors.script.<name>]`** config section — TOML config with `path`, `timeout`, and arbitrary keys passed to the Lua script. Values support `${VAR_NAME}` environment variable expansion.
- Example GitHub Issues connector (`examples/connectors/github-issues.lua`).
- Test fixture connector (`tests/fixtures/test_connector.lua`).

- **Rust extension traits** — define custom connectors and tools in compiled Rust via `Connector` and `Tool` traits. Register with `ConnectorRegistry` and `ToolRegistry`, then use `run_server_with_extensions()` and `run_sync_with_extensions()` to integrate them alongside built-in and Lua extensions. Custom connectors sync via `ctx sync custom:<name>`. Custom tools appear in `GET /tools/list` and execute via `POST /tools/{name}`. Includes `ToolContext` bridge for search/get/sources access during tool execution.
- **`async-trait`** dependency — used for `dyn Trait` async method dispatch in `Connector` and `Tool` traits.

- **Named multi-instance connectors** — all connector types (filesystem, git, s3) now use named instances (`[connectors.git.platform]`, `[connectors.filesystem.docs]`, etc.) matching the existing script connector pattern. Configure multiple of each type. Documents are tagged with `source = "type:name"` (e.g. `"git:platform"`).
- **`ctx sync all`** — sync every configured connector in parallel via `tokio::task::JoinSet`.
- **`ctx sync <type>`** — sync all instances of a type (e.g. `ctx sync git` syncs all git connectors).
- **`ctx sync <type>:<name>`** — sync a specific named instance.
- **Parallel scanning** — when syncing multiple connectors, all scans run concurrently. SQLite writes remain serial for consistency.

### Changed
- **Documentation site rebuilt** — replaced the browser-based search/chat demo with a clean, static documentation site covering getting started, configuration, CLI reference, HTTP API, search & retrieval, Cursor/MCP integration, CI/CD, and deployment. All content is committed as static HTML — no build step needed for docs.
- **Simplified `build-docs.sh`** — now only generates rustdoc API reference. The docs page is static HTML.

### Dependencies
- Added `mlua` (Lua 5.4 vendored + send) for scripted connector runtime.
- Added `base64` for base64 encoding/decoding in Lua host API.
- Added `blocking` feature to `reqwest` for synchronous HTTP in Lua scripts.

### Previously Added
- **Git connector** — ingest documents from any Git repository (local or remote), with support for branch selection, subdirectory scoping, shallow clones, and glob filtering. Use `ctx sync git`.
- **S3 connector** — ingest documents from Amazon S3 buckets with prefix filtering, glob matching, and AWS credential resolution. Use `ctx sync s3`.
- **Rustdoc API reference** — full API documentation generated from source and deployed to `site/api/`.
- **Library target** (`src/lib.rs`) — re-exports all modules as public for rustdoc generation and potential reuse as a library.
- **Deployment guide** (`docs/DEPLOYMENT.md`) — comprehensive documentation covering building from source, local development, production deployment (systemd, Docker, cron), CI/CD pipelines, Cursor/MCP integration, and troubleshooting.
- Thorough rustdoc documentation on every public and private struct, function, trait, and module across the entire codebase.
- This CHANGELOG file.

## [0.1.0] — 2026-02-21

### Added

#### Phase 1 — Core CLI & Filesystem Connector
- `ctx init` — initialize SQLite database with schema migrations (idempotent).
- `ctx sources` — list available connectors and their health status.
- `ctx sync filesystem` — ingest files from a local directory with glob-based include/exclude, incremental sync via checkpoints, `--full`, `--dry-run`, `--since`, `--until`, `--limit` flags.
- `ctx search "<query>"` — full-text keyword search over indexed chunks (FTS5/BM25).
- `ctx get <id>` — retrieve a document by UUID with all chunks.
- Paragraph-boundary text chunker with configurable `max_tokens`.
- TOML-based configuration (`ctx.toml`).
- GitHub Actions CI workflow (fmt, clippy, test).
- GitHub Pages marketing site with animated terminal demo.
- MIT license, README, CONTRIBUTING guide.

#### Phase 2 — Embeddings & Hybrid Search
- Embedding provider abstraction (`EmbeddingProvider` trait) with `Disabled` and `OpenAI` implementations.
- OpenAI embedding provider with batching, retry/backoff, timeouts, and `OPENAI_API_KEY` support.
- `ctx embed pending` — backfill missing/stale embeddings with `--limit`, `--batch-size`, `--dry-run`.
- `ctx embed rebuild` — delete and regenerate all embeddings.
- Inline embedding during `ctx sync` (non-fatal on failure).
- Staleness detection via SHA-256 hash of chunk text.
- `embeddings` and `chunk_vectors` (sqlite-vec) tables.
- `--mode semantic` — pure vector search via cosine similarity.
- `--mode hybrid` — weighted merge of keyword + semantic results with min-max normalization (configurable `hybrid_alpha`).
- Comprehensive Phase 2 integration tests.

#### Phase 3 — MCP Server
- `ctx serve mcp` — start an MCP-compatible HTTP server (Axum).
- `POST /tools/search` — search endpoint with mode, filters, limit.
- `POST /tools/get` — document retrieval endpoint.
- `GET /tools/sources` — connector status endpoint.
- `GET /health` — health check.
- CORS enabled for cross-origin requests.
- Structured error responses (`bad_request`, `not_found`, `embeddings_disabled`, `internal`).
- Cursor MCP integration configuration.
- Phase 3 integration tests (8 tests).

#### Demo & Browser Demo
- Interactive demo project (`demo/`) with 11-document Acme Engineering Handbook.
- Demo web UI with search mode switching, result annotations, and learning explanations.
- "About the Demo" educational page.
- `demo.sh` script for automated setup and launch.
- Browser-only demo (`site/demo/`) running entirely client-side with sql.js, Transformers.js (WASM), and BM25 in JavaScript.
- Use Cases section on the marketing page.

