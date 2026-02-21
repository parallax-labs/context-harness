mod chunk;
mod config;
mod connector_fs;
mod db;
mod get;
mod ingest;
mod migrate;
mod models;
mod search;
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

        /// Search mode
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
    }

    Ok(())
}
