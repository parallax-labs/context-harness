//! # Context Harness
//!
//! **A local-first context ingestion and retrieval framework for AI tools.**
//!
//! Context Harness provides a connector-driven pipeline for ingesting documents
//! from multiple sources (filesystem, Git repositories, S3 buckets, Lua scripts), chunking
//! and embedding them, and exposing hybrid search (keyword + semantic) via a
//! CLI and MCP-compatible HTTP server.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐   ┌─────────────┐   ┌──────────┐
//! │ Connectors  │──▶│  Pipeline    │──▶│  SQLite   │
//! │ FS/Git/S3   │   │ Chunk+Embed │   │ FTS5+Vec  │
//! └─────────────┘   └─────────────┘   └────┬─────┘
//!                                          │
//!                      ┌───────────────────┤
//!                      ▼                   ▼
//!                 ┌──────────┐       ┌──────────┐
//!                 │   CLI    │       │   HTTP   │
//!                 │  (ctx)   │       │  (MCP)   │
//!                 └──────────┘       └──────────┘
//! ```
//!
//! ## Data Flow
//!
//! 1. **Connectors** scan external sources and produce [`models::SourceItem`]s.
//! 2. The **ingestion pipeline** ([`ingest`]) normalizes items into [`models::Document`]s,
//!    computes deduplication hashes, and upserts them into SQLite.
//! 3. Documents are split into [`models::Chunk`]s by the paragraph-boundary
//!    chunker ([`chunk`]).
//! 4. Chunks are indexed in **FTS5** for keyword search and optionally
//!    embedded via the **embedding provider** ([`embedding`]) for vector search.
//! 5. The **query engine** ([`search`]) supports keyword, semantic, and hybrid
//!    retrieval with min-max normalized scoring.
//! 6. Results are exposed via the **CLI** (`ctx`) and the **MCP HTTP server** ([`server`]).
//!
//! ## Quick Start
//!
//! ```bash
//! ctx init                      # create database
//! ctx sync all                  # ingest all configured sources (parallel)
//! ctx sync git:platform         # ingest a specific git connector
//! ctx embed pending             # generate embeddings
//! ctx search "deployment" --mode hybrid
//! ctx serve mcp                 # start HTTP server
//! ```
//!
//! ## Connectors
//!
//! | Connector | Source | Module |
//! |-----------|--------|--------|
//! | Filesystem | Local directories | [`connector_fs`] |
//! | Git | Any Git repository (local or remote) | [`connector_git`] |
//! | S3 | Amazon S3 / S3-compatible buckets | [`connector_s3`] |
//! | Lua Script | Any source via custom Lua scripts | [`connector_script`] |
//!
//! ## Search Modes
//!
//! | Mode | Engine | Requires Embeddings |
//! |------|--------|---------------------|
//! | `keyword` | SQLite FTS5 (BM25) | No |
//! | `semantic` | Cosine similarity over vectors | Yes |
//! | `hybrid` | Weighted merge (configurable α) | Yes |
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`config`] | TOML configuration parsing and validation |
//! | [`models`] | Core data types: `SourceItem`, `Document`, `Chunk`, `SearchResult` |
//! | [`connector_fs`] | Filesystem connector: walk local directories |
//! | [`connector_git`] | Git connector: clone/pull repos with per-file metadata |
//! | [`connector_s3`] | S3 connector: list and download objects with SigV4 signing |
//! | [`connector_script`] | Lua scripted connectors: custom data sources via Lua 5.4 scripts |
//! | [`lua_runtime`] | Shared Lua 5.4 VM runtime: sandboxing, host APIs, value conversions |
//! | [`tool_script`] | Lua MCP tool extensions: load, validate, execute Lua tool scripts |
//! | [`traits`] | Extension traits: `Connector`, `Tool`, `ToolContext`, registries |
//! | [`agents`] | Agent system: `Agent` trait, `AgentPrompt`, `AgentRegistry`, `TomlAgent` |
//! | [`agent_script`] | Lua scripted agents: load, resolve, scaffold, test |
//! | [`chunk`] | Paragraph-boundary text chunker |
//! | [`embedding`] | Embedding provider trait, OpenAI implementation, vector utilities |
//! | [`embed_cmd`] | Embedding CLI commands: `pending` and `rebuild` |
//! | [`export`] | JSON export for static site search (`ctx export`) |
//! | [`stats`] | Database statistics: document, chunk, and embedding counts |
//! | [`ingest`] | Ingestion pipeline: connector → normalize → chunk → embed → store |
//! | [`search`] | Keyword, semantic, and hybrid search with score normalization |
//! | [`get`] | Document retrieval by UUID |
//! | [`sources`] | Connector health and status listing |
//! | [`server`] | MCP-compatible HTTP server (Axum) with CORS |
//! | [`db`] | SQLite connection pool with WAL mode |
//! | [`migrate`] | Database schema migrations (idempotent) |
//!
//! ## Configuration
//!
//! Context Harness is configured via a TOML file (default: `config/ctx.toml`).
//! See [`config`] for all available options and [`config::load_config`] for
//! validation rules.

pub mod agent_script;
pub mod agents;
pub mod chunk;
pub mod config;
pub mod connector_fs;
pub mod connector_git;
pub mod connector_s3;
pub mod connector_script;
pub mod db;
pub mod embed_cmd;
pub mod embedding;
pub mod export;
pub mod extract;
pub mod get;
pub mod ingest;
pub mod lua_runtime;
pub mod mcp;
pub mod migrate;
pub mod models;
pub mod progress;
pub mod registry;
pub mod search;
pub mod server;
pub mod sources;
pub mod sqlite_store;
pub mod stats;
pub mod tool_script;
pub mod traits;

pub use agents::{Agent, AgentPrompt, AgentRegistry, TomlAgent};
pub use context_harness_core::store;
pub use models::SourceItem;
pub use traits::{
    Connector, ConnectorRegistry, GetTool, SearchTool, SourcesTool, Tool, ToolContext, ToolRegistry,
};
