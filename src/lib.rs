//! # Context Harness
//!
//! A local-first context ingestion and retrieval framework for AI tools.
//!
//! Context Harness provides a connector-driven pipeline for ingesting documents
//! from multiple sources (filesystem, Git repositories, S3 buckets), chunking
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
//! ## Quick Start
//!
//! ```bash
//! ctx init                      # create database
//! ctx sync filesystem           # ingest local files
//! ctx sync git                  # ingest from a git repo
//! ctx embed pending             # generate embeddings
//! ctx search "deployment" --mode hybrid
//! ctx serve mcp                 # start HTTP server
//! ```
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`config`] | TOML configuration parsing |
//! | [`models`] | Core data types |
//! | [`connector_fs`] | Filesystem connector |
//! | [`connector_git`] | Git repository connector |
//! | [`connector_s3`] | Amazon S3 connector |
//! | [`chunk`] | Text chunking |
//! | [`embedding`] | Embedding provider abstraction |
//! | [`search`] | Keyword, semantic, and hybrid search |
//! | [`server`] | MCP HTTP server |
//! | [`db`] | Database connection |
//! | [`migrate`] | Schema migrations |

pub mod chunk;
pub mod config;
pub mod connector_fs;
pub mod connector_git;
pub mod connector_s3;
pub mod db;
pub mod embed_cmd;
pub mod embedding;
pub mod get;
pub mod ingest;
pub mod migrate;
pub mod models;
pub mod search;
pub mod server;
pub mod sources;
