# PRD-0001: Core Context Engine

**Status:** Delivered
**Date:** 2026-02-19 (conceived) / 2026-02-27 (documented)
**Author:** pjones

## Problem Statement

LLM-powered developer tools (Cursor, Claude Desktop, Continue.dev) have no
memory of an organization's context. They can read the file you are editing
and grep a few things, but they do not know:

- Architecture decisions and the reasoning behind them
- Incident runbooks and what failed last time
- API contracts across services
- Deployment playbooks and their gotchas
- Jira tickets, Confluence pages, Slack threads, and other tribal knowledge

This gets worse at scale. Fifteen repos, three teams, documentation scattered
across systems. Engineers copy-paste context into prompts. That does not scale.

Cloud RAG services exist but are complex, expensive, and require sending
data off-device. There is no lightweight, local-first tool that ingests
multiple sources, indexes them, and exposes unified retrieval to AI tools.

## Target Users

1. **Individual developers** using AI coding tools (Cursor, Claude Desktop)
   who want project-specific context in their agent conversations.
2. **Small teams** with multiple repos, docs, and knowledge scattered across
   systems who need cross-repo search.
3. **Self-hosters** who want their data to stay on their machine or trusted
   network -- no cloud dependencies required.

## Goals

1. Ingest documents from arbitrary systems (filesystem, Git repos, S3,
   custom scripts) into a single local index.
2. Normalize all content into a consistent Document model with deterministic
   upserts and incremental sync.
3. Chunk documents intelligently (paragraph-boundary aware, configurable
   token limits) for retrieval quality.
4. Embed chunks with configurable providers (OpenAI, Ollama, local) for
   semantic search.
5. Store everything in SQLite -- single file, no infrastructure, trivial
   backup.
6. Provide hybrid search combining FTS5 keyword scoring (BM25) and cosine
   vector similarity with configurable blending.
7. Expose retrieval via CLI (`ctx search`, `ctx get`, `ctx sources`) and
   an MCP-compatible HTTP server (`ctx serve mcp`).
8. Support multi-source configuration so one `ctx.toml` indexes multiple
   repos and systems into a shared database.
9. Ship as a single compiled Rust binary with no runtime dependencies.

## Non-Goals

- Multi-user cloud service or SaaS deployment.
- Real-time streaming guarantees.
- Perfect deduplication across all sources.
- Built-in chat UI or LLM integration (the tool serves context; the LLM
  is the client).
- Replacing dedicated vector databases for million-document-scale workloads.

## User Stories

**US-1: Index local repos for Cursor.**
A developer configures `ctx.toml` with three Git repos (frontend, backend,
platform). They run `ctx sync git --full && ctx embed pending`. They start
`ctx serve mcp` and add it to Cursor's MCP settings. When they ask Cursor
"how does the auth service validate tokens?", Cursor calls the search tool
and gets relevant architecture docs and code from all three repos.

**US-2: Keyword search from the terminal.**
A developer runs `ctx search "jwt signing key" --mode keyword` and gets
back matching documents ranked by BM25 score, with source, title, and
snippet. No embeddings required.

**US-3: Hybrid search with scoring explanation.**
A developer runs `ctx search "how do we handle auth" --mode hybrid --explain`
and sees each result with its keyword score, semantic score, combined hybrid
score, and the alpha weight used.

**US-4: Multi-format ingestion.**
A developer points a filesystem connector at a directory containing Markdown,
PDF, and .docx files. After sync, all documents are searchable -- PDFs and
Office docs have their text extracted automatically.

**US-5: Retrieve a full document.**
An MCP client calls `context.get` with a document ID and receives the full
body, metadata, and chunk list.

## Requirements

1. The system SHALL ingest documents from filesystem, Git, and S3 connectors.
2. The system SHALL normalize all content into a Document model with: id,
   source, source_id, title, body, metadata, content hash, and timestamps.
3. The system SHALL chunk documents at paragraph boundaries with configurable
   maximum token count and overlap.
4. The system SHALL store documents, chunks, and embeddings in a single
   SQLite database using WAL mode.
5. The system SHALL provide keyword search via FTS5 with BM25 scoring.
6. The system SHALL provide semantic search via cosine similarity over
   stored embedding vectors.
7. The system SHALL provide hybrid search that normalizes keyword and
   semantic scores (min-max), blends them with a configurable alpha, and
   aggregates to document level using MAX.
8. The system SHALL expose search, get, and sources via CLI subcommands.
9. The system SHALL expose search, get, and sources via an MCP-compatible
   HTTP server using Streamable HTTP transport.
10. The system SHALL support TOML configuration with environment variable
    expansion for secrets.
11. The system SHALL extract text from PDF, .docx, .pptx, and .xlsx files
    during ingestion.
12. The system SHALL perform deterministic upserts based on content hash
    to avoid re-processing unchanged documents.
13. The system SHALL build and run as a single static binary on Linux
    (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64).

## Success Criteria

- Can ingest 3+ repos and search across all of them in a single query.
- Keyword search returns results in <10ms for ~50K chunks.
- Hybrid search returns results in <50ms with local embeddings.
- CLI and MCP server produce identical search results for the same query.
- Cursor can connect via MCP and retrieve context during conversations.
- The entire knowledge base is a single `.sqlite` file that can be copied
  or backed up trivially.

## Dependencies and Risks

- **SQLite performance ceiling:** For personal/team-scale (tens of thousands
  of documents), SQLite is sufficient. Million-document workloads would
  need a different backend (addressed by PRD-0006).
- **Embedding provider availability:** If no API key is configured and
  local embeddings are not built, semantic and hybrid search are unavailable.
  Keyword-only mode remains functional (addressed by PRD-0004).
- **Cross-platform builds:** Nix flake handles reproducible builds across
  six targets. Non-Nix users can build with standard `cargo build`.

## Related Documents

- **ADRs:** [0001](../adr/0001-rust-as-implementation-language.md),
  [0002](../adr/0002-sqlite-as-embedded-storage.md),
  [0003](../adr/0003-fts5-for-keyword-search.md),
  [0004](../adr/0004-brute-force-vector-search.md),
  [0005](../adr/0005-hybrid-scoring-with-min-max-normalization.md),
  [0006](../adr/0006-paragraph-boundary-chunking.md),
  [0009](../adr/0009-mcp-streamable-http-transport.md),
  [0010](../adr/0010-toml-configuration-with-env-expansion.md),
  [0012](../adr/0012-pure-rust-s3-client.md),
  [0016](../adr/0016-nix-for-builds.md),
  [0017](../adr/0017-rustls-over-openssl.md)
- **Specs:** [HYBRID_SCORING.md](../HYBRID_SCORING.md),
  [FILE_SUPPORT.md](../FILE_SUPPORT.md),
  [SYNC_PROGRESS.md](../SYNC_PROGRESS.md),
  [SCHEMAS.md](../SCHEMAS.md),
  [USAGE.md](../USAGE.md)
- **Design:** [DESIGN.md](../DESIGN.md),
  [DEPLOYMENT.md](../DEPLOYMENT.md)
