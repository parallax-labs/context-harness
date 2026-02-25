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

mod agent_script;
mod agents;
mod chunk;
mod config;
mod connector_fs;
mod connector_git;
mod connector_s3;
mod connector_script;
mod db;
mod embed_cmd;
mod embedding;
mod export;
mod get;
mod ingest;
mod lua_runtime;
mod mcp;
mod migrate;
mod models;
mod registry;
mod search;
mod server;
mod sources;
mod stats;
mod tool_script;
#[allow(dead_code)]
mod traits;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
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

    /// Show database statistics.
    ///
    /// Displays document, chunk, and embedding counts with a per-source
    /// breakdown and last sync timestamps. Useful for verifying that
    /// syncs and embeddings completed successfully.
    Stats,

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
    ///
    /// Connector format: `all`, `<type>`, or `<type>:<name>`.
    /// Examples: `all`, `git`, `git:platform`, `filesystem:docs`, `s3:runbooks`.
    Sync {
        /// Connector specifier: `all`, a type (`git`, `filesystem`, `s3`, `script`),
        /// or a specific instance (`git:platform`).
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

        /// Show scoring breakdown per result (keyword, semantic, hybrid scores and alpha).
        #[arg(long)]
        explain: bool,
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

    /// Manage Lua connector scripts.
    ///
    /// Create, test, and debug Lua connector scripts that extend
    /// Context Harness with custom data sources.
    Connector {
        #[command(subcommand)]
        action: ConnectorAction,
    },

    /// Manage Lua tool scripts.
    ///
    /// Create, test, and list Lua tool scripts that expose custom MCP tools
    /// for AI agents to discover and call.
    Tool {
        #[command(subcommand)]
        action: ToolAction,
    },

    /// Manage agents (personas with system prompts and tool scoping).
    ///
    /// Create, test, and list agents that provide "assume a role" workflows
    /// for Cursor, Claude, and other MCP clients.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Manage extension registries (community connectors, tools, agents).
    ///
    /// Install, update, search, and scaffold config entries for extensions
    /// from Git-backed registries.
    Registry {
        #[command(subcommand)]
        action: RegistryAction,
    },

    /// Generate shell completions for bash, zsh, or fish.
    ///
    /// Prints completion script to stdout. Redirect to the appropriate
    /// file for your shell.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },

    /// Export the search index as a JSON file for static site search.
    ///
    /// Exports all documents and chunks to a JSON file that can be
    /// used with `ctx-search.js` for client-side search on static sites.
    Export {
        /// Output file path (defaults to stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
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

/// Connector management subcommands.
#[derive(Subcommand)]
enum ConnectorAction {
    /// Test a Lua connector script without writing to the database.
    ///
    /// Loads the script, executes `connector.scan()`, and prints the
    /// returned items. Useful for development and debugging.
    Test {
        /// Path to the `.lua` connector script.
        path: PathBuf,
        /// Use config from a named script connector entry.
        #[arg(long)]
        source: Option<String>,
    },
    /// Scaffold a new connector from a template.
    ///
    /// Creates `connectors/<name>.lua` with a commented template.
    Init {
        /// Name for the new connector (e.g., `jira`, `confluence`).
        name: String,
    },
}

/// Tool management subcommands.
#[derive(Subcommand)]
enum ToolAction {
    /// Test a Lua tool script with sample parameters.
    ///
    /// Loads the script, executes `tool.execute()` with the given parameters,
    /// and prints the result. Useful for development and debugging.
    Test {
        /// Path to the `.lua` tool script.
        path: PathBuf,
        /// Tool parameters as `key=value` pairs.
        #[arg(long = "param", value_parser = parse_key_val)]
        params: Vec<(String, String)>,
        /// Use config from a named tool entry in ctx.toml.
        #[arg(long)]
        source: Option<String>,
    },
    /// Scaffold a new tool from a template.
    ///
    /// Creates `tools/<name>.lua` with a commented template.
    Init {
        /// Name for the new tool (e.g., `create_jira_ticket`).
        name: String,
    },
    /// List all configured tools (built-in and Lua).
    List,
}

/// Agent management subcommands.
#[derive(Subcommand)]
enum AgentAction {
    /// List all configured agents (TOML and Lua).
    List,
    /// Test an agent by resolving its prompt.
    ///
    /// Loads the agent, calls its `resolve()` function with the provided
    /// arguments, and prints the resulting system prompt and messages.
    Test {
        /// Agent name (as defined in `[agents.inline.<name>]` or `[agents.script.<name>]`).
        name: String,
        /// Agent arguments as `key=value` pairs.
        #[arg(long = "arg", value_parser = parse_key_val)]
        args: Vec<(String, String)>,
    },
    /// Scaffold a new Lua agent script from a template.
    ///
    /// Creates `agents/<name>.lua` with a commented template showing
    /// the agent interface.
    Init {
        /// Name for the new agent (e.g., `code-reviewer`).
        name: String,
    },
}

/// Registry management subcommands.
#[derive(Subcommand)]
enum RegistryAction {
    /// List configured registries and available extensions.
    List,
    /// Install (clone) configured registries.
    ///
    /// Clones git-backed registries that aren't yet present on disk.
    Install {
        /// Specific registry name, or all if omitted.
        name: Option<String>,
    },
    /// Update (git pull) registries.
    ///
    /// Pulls the latest changes for git-backed registries. Skips registries
    /// with uncommitted changes.
    Update {
        /// Specific registry name, or all if omitted.
        name: Option<String>,
    },
    /// Search extensions by name, tag, or description.
    Search {
        /// Search query (matches against name, description, and tags).
        query: String,
    },
    /// Show details for a specific extension.
    Info {
        /// Extension identifier (e.g. `connectors/jira`, `tools/summarize`).
        extension: String,
    },
    /// Scaffold a config entry for an extension in ctx.toml.
    ///
    /// Reads the extension's `config.example.toml` (if present) and appends
    /// a ready-to-fill section to your config file.
    Add {
        /// Extension identifier (e.g. `connectors/jira`, `tools/summarize`).
        extension: String,
    },
    /// Copy an extension to a writable registry for customization.
    ///
    /// Creates a local override that takes precedence over the original.
    Override {
        /// Extension identifier (e.g. `connectors/jira`).
        extension: String,
    },
    /// Install the community extension registry (first-run setup).
    Init,
}

/// Parse a `key=value` pair for `--param` arguments.
fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=VALUE: no '=' found in '{}'", s))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
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

    // Commands that don't require config
    match &cli.command {
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(*shell, &mut cmd, "ctx", &mut std::io::stdout());
            return Ok(());
        }
        Commands::Connector {
            action: ConnectorAction::Init { name },
        } => {
            connector_script::scaffold_connector(name)?;
            return Ok(());
        }
        Commands::Connector {
            action: ConnectorAction::Test { path, source },
        } => {
            // Use config if available, otherwise a minimal default
            let cfg =
                config::load_config(&cli.config).unwrap_or_else(|_| config::Config::minimal());
            connector_script::test_script(path, &cfg, source.as_deref()).await?;
            return Ok(());
        }
        Commands::Tool {
            action: ToolAction::Init { name },
        } => {
            tool_script::scaffold_tool(name)?;
            return Ok(());
        }
        Commands::Agent {
            action: AgentAction::Init { name },
        } => {
            agent_script::scaffold_agent(name)?;
            return Ok(());
        }
        Commands::Registry {
            action: RegistryAction::Init,
        } => {
            registry::cmd_init_community(&cli.config)?;
            return Ok(());
        }
        Commands::Registry {
            action: RegistryAction::Install { ref name },
        } => {
            let cfg =
                config::load_config(&cli.config).unwrap_or_else(|_| config::Config::minimal());
            registry::cmd_install(&cfg, name.as_deref())?;
            return Ok(());
        }
        Commands::Registry {
            action: RegistryAction::Update { ref name },
        } => {
            let cfg =
                config::load_config(&cli.config).unwrap_or_else(|_| config::Config::minimal());
            registry::cmd_update(&cfg, name.as_deref())?;
            return Ok(());
        }
        Commands::Tool {
            action: ToolAction::Test { path, source, .. },
        } if source.is_none() => {
            // Without --source, use minimal config
            let cfg =
                config::load_config(&cli.config).unwrap_or_else(|_| config::Config::minimal());
            if let Commands::Tool {
                action:
                    ToolAction::Test {
                        path,
                        params,
                        source,
                    },
            } = cli.command
            {
                tool_script::test_tool(&path, params, &cfg, source.as_deref()).await?;
            }
            return Ok(());
        }
        _ => {}
    }

    let cfg = config::load_config(&cli.config)?;

    match cli.command {
        Commands::Init => {
            migrate::run_migrations(&cfg).await?;
            println!("Database initialized successfully.");

            // Offer to install the community registry if not already configured
            if cfg.registries.is_empty() && atty::is(atty::Stream::Stdin) {
                eprint!("Would you like to install the community extension registry? [Y/n] ");
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_ok() {
                    let answer = input.trim().to_lowercase();
                    if answer.is_empty() || answer == "y" || answer == "yes" {
                        if let Err(e) = registry::cmd_init_community(&cli.config) {
                            eprintln!("Warning: failed to install community registry: {}", e);
                        }
                    }
                }
            }
        }
        Commands::Stats => {
            stats::run_stats(&cfg).await?;
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
            explain,
        } => {
            search::run_search(&cfg, &query, &mode, source, since, limit, explain).await?;
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
        Commands::Connector { action } => match action {
            ConnectorAction::Test { path, source } => {
                connector_script::test_script(&path, &cfg, source.as_deref()).await?;
            }
            ConnectorAction::Init { .. } => {
                // Handled above (before config loading)
                unreachable!()
            }
        },
        Commands::Tool { action } => match action {
            ToolAction::Test {
                path,
                params,
                source,
            } => {
                tool_script::test_tool(&path, params, &cfg, source.as_deref()).await?;
            }
            ToolAction::List => {
                tool_script::list_tools(&cfg)?;
            }
            ToolAction::Init { .. } => {
                // Handled above (before config loading)
                unreachable!()
            }
        },
        Commands::Export { output } => {
            export::run_export(&cfg, output.as_deref()).await?;
        }
        Commands::Registry { action } => match action {
            RegistryAction::List => {
                registry::cmd_list(&cfg);
            }
            RegistryAction::Search { query } => {
                registry::cmd_search(&cfg, &query);
            }
            RegistryAction::Info { extension } => {
                registry::cmd_info(&cfg, &extension)?;
            }
            RegistryAction::Add { extension } => {
                registry::cmd_add(&cfg, &extension, &cli.config)?;
            }
            RegistryAction::Override { extension } => {
                registry::cmd_override(&cfg, &extension)?;
            }
            RegistryAction::Install { .. }
            | RegistryAction::Update { .. }
            | RegistryAction::Init => {
                unreachable!()
            }
        },
        Commands::Completions { .. } => unreachable!(),
        Commands::Agent { action } => match action {
            AgentAction::List => {
                agent_script::list_agents(&cfg)?;
            }
            AgentAction::Test { name, args } => {
                agent_script::test_agent(&name, args, &cfg).await?;
            }
            AgentAction::Init { .. } => {
                // Handled above (before config loading)
                unreachable!()
            }
        },
    }

    Ok(())
}
