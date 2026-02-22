# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Chat with Docs** — RAG-powered chat interface in the docs page. Ask questions in natural language and get answers grounded in the documentation with source citations.
  - **WebLLM backend** — fully offline LLM inference via WebGPU (Phi-3.5-mini). Model cached in IndexedDB after first download (~1.4GB).
  - **OpenAI API backend** — paste your own API key for gpt-4o-mini. Key stays in localStorage, never leaves the browser.
  - Hybrid search retrieval (BM25 + semantic) feeds top-5 relevant chunks as context to the LLM.
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
- Comprehensive rustdoc module-level comments across all source modules.
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

