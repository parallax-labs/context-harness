# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Agent Tool Calling** — the browser chat is now a full agent that calls Context Harness MCP tools (`search`, `get_document`, `list_sources`) in a multi-turn loop, executing them client-side or against a local MCP server. This is the same tool protocol used by Cursor and other AI editors.
  - Tool call cards shown inline with collapsible results — watch the agent search, retrieve, and synthesize.
  - Max 6 tool-calling rounds per question with automatic fallback to one-shot RAG.
  - Streaming response with real-time tool call detection for both OpenAI and WebLLM backends.
  - Optional **MCP Server connection** — connect to a running `ctx serve mcp` instance for live data.
  - Client-side tool execution (offline) as default — tools run in the browser using `data.json`.
  - Settings panel for tool execution mode (Browser/MCP) with server URL and connection testing.
- **Chat with Docs** — RAG-powered chat interface in the docs page. Ask questions in natural language and get answers grounded in the documentation with source citations.
  - **WebLLM backend** — fully offline LLM inference via WebGPU (Qwen3-4B). Model cached in IndexedDB after first download (~2.5GB).
  - **OpenAI API backend** — paste your own API key for gpt-4o-mini. Key stays in localStorage, never leaves the browser.
  - Hybrid search retrieval (BM25 + semantic) feeds relevant chunks as context to the LLM.
  - Streaming token display with Markdown rendering (code blocks, headers, lists, bold/italic).
  - Source document links appended to each answer for easy navigation.
  - Conversation history (last 6 exchanges) for follow-up questions.
  - Settings panel to switch backends and configure API key.
- **Git connector** — ingest documents from any Git repository (local or remote), with support for branch selection, subdirectory scoping, shallow clones, and glob filtering. Use `ctx sync git`.
- **S3 connector** — ingest documents from Amazon S3 buckets with prefix filtering, glob matching, and AWS credential resolution. Use `ctx sync s3`.
- **Documentation site** — searchable docs at `site/docs/`, dogfooding the Git connector to index the repo's own guides and source code. Built automatically in CI.
- **Rustdoc API reference** — full API documentation generated from source and deployed to `site/api/`.
- **Library target** (`src/lib.rs`) — re-exports all modules as public for rustdoc generation and potential reuse as a library.
- **`scripts/build-docs.sh`** — build script that uses the Git connector to ingest documentation, generate rustdoc, export search data, and prepare the site for deployment.
- **Deployment guide** (`docs/DEPLOYMENT.md`) — comprehensive documentation covering building from source, local development, production deployment (systemd, Docker, cron), CI/CD pipelines, Cursor/MCP integration, browser-only demo, and troubleshooting.
- Thorough rustdoc documentation on every public and private struct, function, trait, and module across the entire codebase: `models.rs`, `config.rs`, `chunk.rs`, `db.rs`, `migrate.rs`, `connector_fs.rs`, `connector_git.rs`, `connector_s3.rs`, `embedding.rs`, `embed_cmd.rs`, `search.rs`, `server.rs`, `ingest.rs`, `get.rs`, `sources.rs`, `main.rs`, and `lib.rs`.
- Enhanced crate-level documentation in `lib.rs` with architecture diagrams, data flow descriptions, connector tables, search mode comparison, and module reference.
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

