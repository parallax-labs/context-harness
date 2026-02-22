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
//! Connectors → Normalization → Chunking → Embedding → SQLite → Query Engine → CLI / MCP Server
//! ```
//!
//! ## Modules
//!
//! - [`config`] — TOML configuration parsing and validation
//! - [`models`] — Core data types: `SourceItem`, `Document`, `Chunk`, `SearchResult`
//! - [`connector_fs`] — Filesystem connector: walk local directories
//! - [`connector_git`] — Git connector: clone/pull repos, walk files with git metadata
//! - [`connector_s3`] — S3 connector: list and download objects from S3 buckets
//! - [`chunk`] — Paragraph-boundary text chunker
//! - [`embedding`] — Embedding provider trait, OpenAI implementation, vector utilities
//! - [`embed_cmd`] — Embedding CLI commands (pending, rebuild)
//! - [`ingest`] — Ingestion pipeline orchestration
//! - [`search`] — Keyword, semantic, and hybrid search
//! - [`get`] — Document retrieval by ID
//! - [`sources`] — Connector health/status listing
//! - [`server`] — MCP-compatible HTTP server (Axum)
//! - [`db`] — SQLite connection management
//! - [`migrate`] — Database schema migrations

mod chunk;
mod config;
mod connector_fs;
mod connector_git;
mod connector_s3;
mod db;
mod embed_cmd;
mod embedding;
mod get;
mod ingest;
mod migrate;
mod models;
mod search;
mod server;
mod sources;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ctx",
    about = "Context Harness — a local-first context ingestion and retrieval framework for AI tools",
    version
)]
struct Cli {
    /// Path to configuration file
    #[arg(long, global = true, default_value = "./config/ctx.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the database schema
    Init,

    /// List available connectors and their status
    Sources,

    /// Ingest data from a connector
    Sync {
        /// Connector name (e.g. filesystem)
        connector: String,

        /// Ignore checkpoint, reingest everything
        #[arg(long)]
        full: bool,

        /// Show counts without writing
        #[arg(long)]
        dry_run: bool,

        /// Only process files modified after this date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,

        /// Only process files modified before this date (YYYY-MM-DD)
        #[arg(long)]
        until: Option<String>,

        /// Limit number of items processed
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Search indexed documents
    Search {
        /// Search query
        query: String,

        /// Search mode: keyword, semantic, or hybrid
        #[arg(long, default_value = "keyword")]
        mode: String,

        /// Filter by source
        #[arg(long)]
        source: Option<String>,

        /// Filter by date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,

        /// Maximum number of results
        #[arg(long)]
        limit: Option<i64>,
    },

    /// Retrieve a document by ID
    Get {
        /// Document ID (UUID)
        id: String,
    },

    /// Manage embeddings
    Embed {
        #[command(subcommand)]
        action: EmbedAction,
    },

    /// Start the MCP-compatible HTTP server
    Serve {
        #[command(subcommand)]
        service: ServeService,
    },
}

#[derive(Subcommand)]
enum EmbedAction {
    /// Embed chunks that are missing or have stale embeddings
    Pending {
        /// Maximum number of chunks to embed
        #[arg(long)]
        limit: Option<usize>,

        /// Override batch size from config
        #[arg(long)]
        batch_size: Option<usize>,

        /// Show counts without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Delete and regenerate all embeddings
    Rebuild {
        /// Override batch size from config
        #[arg(long)]
        batch_size: Option<usize>,
    },
}

#[derive(Subcommand)]
enum ServeService {
    /// Start the MCP tool server
    Mcp,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = config::load_config(&cli.config)?;

    match cli.command {
        Commands::Init => {
            migrate::run_migrations(&cfg).await?;
            println!("Database initialized successfully.");
        }
        Commands::Sources => {
            sources::list_sources(&cfg)?;
        }
        Commands::Sync {
            connector,
            full,
            dry_run,
            since,
            until,
            limit,
        } => {
            ingest::run_sync(&cfg, &connector, full, dry_run, since, until, limit).await?;
        }
        Commands::Search {
            query,
            mode,
            source,
            since,
            limit,
        } => {
            search::run_search(&cfg, &query, &mode, source, since, limit).await?;
        }
        Commands::Get { id } => {
            get::run_get(&cfg, &id).await?;
        }
        Commands::Embed { action } => match action {
            EmbedAction::Pending {
                limit,
                batch_size,
                dry_run,
            } => {
                embed_cmd::run_embed_pending(&cfg, limit, batch_size, dry_run).await?;
            }
            EmbedAction::Rebuild { batch_size } => {
                embed_cmd::run_embed_rebuild(&cfg, batch_size).await?;
            }
        },
        Commands::Serve { service } => match service {
            ServeService::Mcp => {
                server::run_server(&cfg).await?;
            }
        },
    }

    Ok(())
}
