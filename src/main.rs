//! # Context Harness CLI (`ctx`)
//!
//! The `ctx` binary is the primary interface for Context Harness. It provides
//! commands for database initialization, data ingestion, search, document
//! retrieval, embedding management, and starting the MCP server.
//!
//! ## Usage
//!
//! ```bash
//! ctx --config ./config/ctx.toml <command>
//! ```
//!
//! ## Commands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `ctx init` | Create the SQLite database and run schema migrations |
//! | `ctx sources` | List all connectors and their health status |
//! | `ctx sync <connector>` | Ingest data from a connector (filesystem, git, s3) |
//! | `ctx search "<query>"` | Search indexed documents |
//! | `ctx get <id>` | Retrieve a full document by UUID |
//! | `ctx embed pending` | Backfill missing or stale embeddings |
//! | `ctx embed rebuild` | Delete and regenerate all embeddings |
//! | `ctx serve mcp` | Start the MCP-compatible HTTP server |
//!
//! ## Examples
//!
//! ```bash
//! # Initialize the database
//! ctx init --config ./config/ctx.toml
//!
//! # Ingest from a local docs directory
//! ctx sync filesystem --config ./config/ctx.toml
//!
//! # Ingest from a Git repository
//! ctx sync git --config ./config/ctx.toml
//!
//! # Keyword search
//! ctx search "authentication flow" --config ./config/ctx.toml
//!
//! # Hybrid search (keyword + semantic)
//! ctx search "deployment" --mode hybrid --config ./config/ctx.toml
//!
//! # Start MCP server for Cursor integration
//! ctx serve mcp --config ./config/ctx.toml
//! ```

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

/// Context Harness CLI — a local-first context ingestion and retrieval
/// framework for AI tools.
///
/// All commands accept a `--config` flag pointing to a TOML configuration
/// file. See `config/ctx.example.toml` for a full example.
#[derive(Parser)]
#[command(
    name = "ctx",
    about = "Context Harness — a local-first context ingestion and retrieval framework for AI tools",
    version,
    long_about = "Context Harness provides a connector-driven pipeline for ingesting documents \
    from multiple sources (filesystem, Git repositories, S3 buckets), chunking and embedding them, \
    and exposing hybrid search (keyword + semantic) via a CLI and MCP-compatible HTTP server."
)]
struct Cli {
    /// Path to configuration file (TOML).
    ///
    /// Defaults to `./config/ctx.toml`. All connector, database, embedding,
    /// and server settings are read from this file.
    #[arg(long, global = true, default_value = "./config/ctx.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

/// Top-level CLI commands.
#[derive(Subcommand)]
enum Commands {
    /// Initialize the database schema.
    ///
    /// Creates the SQLite database file and all required tables
    /// (documents, chunks, checkpoints, chunks_fts, embeddings, chunk_vectors).
    /// This command is idempotent — running it multiple times is safe.
    Init,

    /// List available connectors and their status.
    ///
    /// Shows which connectors are configured and whether they pass
    /// their health checks. Useful for verifying configuration before
    /// running a sync.
    Sources,

    /// Ingest data from a connector.
    ///
    /// Scans the specified connector, normalizes items into documents,
    /// chunks them, optionally embeds them, and stores everything in SQLite.
    /// Supports incremental sync via checkpoints.
    Sync {
        /// Connector name: `filesystem`, `git`, or `s3`.
        connector: String,

        /// Ignore checkpoint — reingest all items from scratch.
        #[arg(long)]
        full: bool,

        /// Dry run — show item and chunk counts without writing to the database.
        #[arg(long)]
        dry_run: bool,

        /// Only process items modified on or after this date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,

        /// Only process items modified on or before this date (YYYY-MM-DD).
        #[arg(long)]
        until: Option<String>,

        /// Maximum number of items to process.
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Search indexed documents.
    ///
    /// Queries the SQLite database using the specified search mode and
    /// returns ranked results with scores and snippets.
    Search {
        /// The search query string.
        query: String,

        /// Search mode: `keyword` (FTS5), `semantic` (vector), or `hybrid` (weighted merge).
        /// Semantic and hybrid modes require an embedding provider to be configured.
        #[arg(long, default_value = "keyword")]
        mode: String,

        /// Filter results to a specific connector source (e.g., `filesystem`, `git`).
        #[arg(long)]
        source: Option<String>,

        /// Only return documents updated on or after this date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,

        /// Maximum number of results to return.
        #[arg(long)]
        limit: Option<i64>,
    },

    /// Retrieve a document by its UUID.
    ///
    /// Prints the document's metadata, full body text, and all chunks.
    Get {
        /// Document UUID.
        id: String,
    },

    /// Manage embedding vectors.
    ///
    /// Subcommands for backfilling, rebuilding, and inspecting embeddings.
    /// Requires an embedding provider (e.g., OpenAI) to be configured.
    Embed {
        #[command(subcommand)]
        action: EmbedAction,
    },

    /// Start the MCP-compatible HTTP server.
    ///
    /// Exposes Context Harness functionality via a JSON API for integration
    /// with Cursor, Claude, and other MCP-compatible AI tools.
    Serve {
        #[command(subcommand)]
        service: ServeService,
    },
}

/// Embedding management subcommands.
#[derive(Subcommand)]
enum EmbedAction {
    /// Embed chunks that are missing or have stale embeddings.
    ///
    /// Finds chunks without embeddings (or with changed text) and generates
    /// new embedding vectors using the configured provider.
    Pending {
        /// Maximum number of chunks to embed in this run.
        #[arg(long)]
        limit: Option<usize>,

        /// Override the batch size from config (number of texts per API call).
        #[arg(long)]
        batch_size: Option<usize>,

        /// Show counts without performing any embedding.
        #[arg(long)]
        dry_run: bool,
    },

    /// Delete and regenerate all embeddings.
    ///
    /// Useful when switching embedding models or dimensions. Clears all
    /// existing vectors and re-embeds every chunk.
    Rebuild {
        /// Override the batch size from config (number of texts per API call).
        #[arg(long)]
        batch_size: Option<usize>,
    },
}

/// Server subcommands.
#[derive(Subcommand)]
enum ServeService {
    /// Start the MCP tool server.
    ///
    /// Binds to the address configured in `[server].bind` and serves
    /// the Context Harness API endpoints.
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
