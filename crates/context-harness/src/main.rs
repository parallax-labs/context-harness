// `async_trait` generates `#[must_use]` futures whose `Result` outputs are
// already `#[must_use]`; Rust 1.99's Clippy reports the generated overlap.
#![allow(clippy::double_must_use)]

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

#[allow(dead_code)]
mod agent_host;
#[allow(dead_code)]
mod agent_model;
mod agent_resource;
#[allow(dead_code)]
mod agent_runtime;
mod agent_script;
#[allow(dead_code)]
mod agent_store;
#[allow(dead_code)]
mod agent_task_store;
mod agents;
mod app_store;
mod chunk;
mod config;
mod connector_fs;
mod connector_git;
mod connector_s3;
mod connector_script;
mod ctx_dirs;
mod db;
mod document_ingestor;
mod embed_cmd;
mod embedding;
mod export;
mod extract;
mod get;
mod ingest;
mod lua_runtime;
mod mcp;
mod migrate;
mod models;
mod profile_script;
mod profiles;
mod progress;
#[allow(dead_code)]
mod redact;
mod registry;
mod search;
mod server;
mod sources;
mod sqlite_store;
mod stats;
#[allow(dead_code)]
mod tool_binding;
mod tool_script;
#[allow(dead_code)]
mod traits;
mod vector_index;
#[allow(dead_code)]
mod workspace;

use anyhow::{ensure, Context};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::path::PathBuf;

use crate::app_store::SqliteAppStore;

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
    /// When omitted, ctx checks CTX_CONFIG, ./.ctx/config.toml,
    /// ./config/ctx.toml, and the XDG global config.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

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

        /// Progress output: `human` (default when stderr is a TTY) or `json` (one JSON object per line on stderr).
        #[arg(long, value_name = "MODE", value_parser = ["human", "json"])]
        progress: Option<String>,

        /// Disable progress output (e.g. for scripts that parse stdout).
        #[arg(long)]
        no_progress: bool,
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

    /// Manage the derived vector-index sidecar.
    VectorIndex {
        #[command(subcommand)]
        action: VectorIndexAction,
    },

    /// Start the MCP-compatible HTTP server.
    ///
    /// Exposes Context Harness functionality via a JSON API for integration
    /// with Cursor, Claude, and other MCP-compatible AI tools.
    Serve {
        #[command(subcommand)]
        service: ServeService,
    },

    /// Manage the multi-workspace registry (`workspaces.toml`).
    ///
    /// Register, list, and remove workspaces served by
    /// `ctx serve mcp --workspaces`. The registry lives at
    /// `$XDG_CONFIG_HOME/ctx/workspaces.toml`.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
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

    /// Manage reusable prompt profiles.
    ///
    /// Create, test, and list personas exposed as MCP prompts. Profiles do not
    /// execute model turns or retain conversation history.
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },

    /// Manage executable local agents.
    ///
    /// Run policy-controlled agents and inspect their durable run history.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Manage extension registries (community connectors, tools, profiles).
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

/// Vector-index management subcommands.
#[derive(Subcommand)]
enum VectorIndexAction {
    /// Show configured backend, sidecar path, and freshness.
    Status,
    /// Rebuild the sidecar from canonical SQLite embeddings.
    Rebuild,
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
    /// Inspect and validate standalone declarative tool bindings.
    Bindings {
        #[command(subcommand)]
        action: ToolBindingsAction,
    },
}

#[derive(Subcommand)]
enum ToolBindingsAction {
    /// List resolved standalone bindings without executing implementations.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one resolved standalone binding.
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Validate standalone binding declarations without executing them.
    Validate {
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

/// Agent management subcommands.
#[derive(Subcommand)]
enum AgentAction {
    /// Execute a standalone agent with policy-controlled local tools.
    Run {
        name: String,
        input: String,
        #[arg(long)]
        json: bool,
        /// Never prompt; deny tool calls that require approval.
        #[arg(long)]
        non_interactive: bool,
    },
    /// Resume an interrupted run from its latest safe checkpoint.
    Resume {
        run_id: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        non_interactive: bool,
    },
    /// Submit durable work for an explicitly started agent worker.
    Enqueue {
        name: String,
        input: String,
        #[arg(long)]
        request_key: String,
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..=10_000))]
        queue_limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Run a foreground worker until interrupted.
    Worker {
        #[arg(long)]
        worker_id: Option<String>,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=32))]
        max_concurrency: u32,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(5..=3600))]
        lease_seconds: u64,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..=3599))]
        heartbeat_seconds: u64,
        #[arg(long, default_value_t = 500, value_parser = clap::value_parser!(u64).range(50..=60_000))]
        poll_ms: u64,
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=100))]
        max_attempts: u32,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=3600))]
        shutdown_seconds: u64,
    },
    /// List this workspace's durable agent jobs.
    Jobs {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Inspect or cancel one durable agent job.
    Job {
        #[command(subcommand)]
        action: AgentJobAction,
    },
    /// List this workspace's durable run history.
    History {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a run and an ordered page of execution events.
    Inspect {
        run_id: String,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i64).range(0..))]
        after_sequence: i64,
        #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// List executable standalone agent resources.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show an agent definition and its resource provenance.
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Validate executable agent resources and model references.
    Validate,
    /// Deprecated alias for `ctx profile test`.
    #[command(hide = true)]
    Test {
        name: String,
        #[arg(long = "arg", value_parser = parse_key_val)]
        args: Vec<(String, String)>,
    },
    /// Deprecated alias for `ctx profile init`.
    #[command(hide = true)]
    Init { name: String },
}

#[derive(Subcommand)]
enum AgentJobAction {
    /// Inspect a job and an ordered page of task events.
    Inspect {
        task_id: String,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i64).range(0..))]
        after_sequence: i64,
        #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Request cancellation of a queued or running job.
    Cancel {
        task_id: String,
        #[arg(long)]
        json: bool,
    },
}

/// Prompt profile management subcommands.
#[derive(Subcommand)]
enum ProfileAction {
    /// List configured profiles and executable-agent prompt projections.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show a profile definition and its provenance.
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Validate profile definitions and agent prompt projections.
    Validate,
    /// Resolve a profile and print its prompt.
    Test {
        name: String,
        #[arg(long = "arg", value_parser = parse_key_val)]
        args: Vec<(String, String)>,
    },
    /// Scaffold a Lua profile in `profiles/<name>.lua`.
    Init { name: String },
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
    ///
    /// Pass `--workspaces` to serve multiple registered workspaces through one
    /// endpoint (multi-workspace mode); without it the server runs in
    /// single-workspace compatibility mode.
    Mcp {
        /// Serve multiple workspaces from the registry (multi-workspace mode).
        ///
        /// Optionally takes a registry path; defaults to
        /// `$XDG_CONFIG_HOME/ctx/workspaces.toml`. Cannot be combined with
        /// `--config` / `CTX_CONFIG`.
        #[arg(long, value_name = "PATH", num_args = 0..=1, require_equals = true)]
        workspaces: Option<Option<PathBuf>>,

        /// Allow binding the multi-workspace server to a non-loopback address.
        ///
        /// Off by default: a non-loopback bind exposes every registered
        /// workspace to other hosts.
        #[arg(long)]
        allow_remote: bool,
    },
}

/// `ctx workspace` subcommands for managing the multi-workspace registry.
#[derive(Subcommand)]
enum WorkspaceAction {
    /// Register a workspace in the registry.
    Add {
        /// Stable workspace id (`[A-Za-z0-9][A-Za-z0-9_-]*`).
        id: String,
        /// Absolute path to the workspace root. Validated at write time.
        #[arg(long)]
        root: PathBuf,
        /// Pin a specific config file as the sole source (no global merge).
        #[arg(long)]
        config: Option<PathBuf>,
        /// Register the workspace as disabled (listed, but rejects queries).
        #[arg(long)]
        disabled: bool,
    },
    /// List registered workspaces.
    List,
    /// Remove a workspace from the registry.
    Remove {
        /// The workspace id to remove.
        id: String,
    },
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
            let cfg = config::load_config_for_cli(cli.config.clone())
                .map(|resolved| resolved.config)
                .unwrap_or_else(|_| config::Config::minimal());
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
            eprintln!("warning: `ctx agent init` is deprecated; use `ctx profile init`");
            profile_script::scaffold_profile(name)?;
            return Ok(());
        }
        Commands::Profile {
            action: ProfileAction::Init { name },
        } => {
            profile_script::scaffold_profile(name)?;
            return Ok(());
        }
        Commands::Registry {
            action: RegistryAction::Init,
        } => {
            let config_path = config::ensure_workspace_config_for_init(cli.config.as_deref())?
                .ok_or_else(|| anyhow::anyhow!("No config path available for registry init"))?;
            registry::cmd_init_community(&config_path)?;
            return Ok(());
        }
        Commands::Registry {
            action: RegistryAction::Install { ref name },
        } => {
            let cfg = config::load_config_for_cli(cli.config.clone())
                .map(|resolved| resolved.config)
                .unwrap_or_else(|_| config::Config::minimal());
            registry::cmd_install(&cfg, name.as_deref())?;
            return Ok(());
        }
        Commands::Registry {
            action: RegistryAction::Update { ref name },
        } => {
            let cfg = config::load_config_for_cli(cli.config.clone())
                .map(|resolved| resolved.config)
                .unwrap_or_else(|_| config::Config::minimal());
            registry::cmd_update(&cfg, name.as_deref())?;
            return Ok(());
        }
        Commands::Tool {
            action: ToolAction::Test { path, source, .. },
        } if source.is_none() => {
            // Without --source, use minimal config
            let cfg = config::load_config_for_cli(cli.config.clone())
                .map(|resolved| resolved.config)
                .unwrap_or_else(|_| config::Config::minimal());
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
        // Multi-workspace MCP serving builds its router from the registry and
        // must NOT resolve a single config (SPEC-0014 R13: --workspaces is
        // incompatible with --config/CTX_CONFIG). Handle it before the
        // single-config resolution below.
        Commands::Serve {
            service:
                ServeService::Mcp {
                    workspaces: Some(reg_opt),
                    allow_remote,
                },
        } => {
            if cli.config.is_some() || std::env::var_os("CTX_CONFIG").is_some() {
                anyhow::bail!(
                    "--workspaces cannot be combined with --config or CTX_CONFIG \
                     (SPEC-0014 R13); drop one of them"
                );
            }
            let reg_path = reg_opt
                .clone()
                .unwrap_or_else(workspace::default_registry_path);
            let registry = workspace::WorkspaceRegistry::load(&reg_path)?;
            let router = std::sync::Arc::new(workspace::build_multi_router(&registry)?);
            let bind = registry
                .defaults
                .bind
                .clone()
                .unwrap_or_else(|| "127.0.0.1:7331".to_string());
            server::run_server_multi(router, bind, *allow_remote).await?;
            return Ok(());
        }
        // The workspace registry commands operate on workspaces.toml and do not
        // need a resolved single config.
        Commands::Workspace { action } => {
            let reg_path = workspace::default_registry_path();
            match action {
                WorkspaceAction::Add {
                    id,
                    root,
                    config,
                    disabled,
                } => {
                    workspace::cmd_add(&reg_path, id, root, config.as_deref(), !*disabled)?;
                }
                WorkspaceAction::List => workspace::cmd_list(&reg_path)?,
                WorkspaceAction::Remove { id } => workspace::cmd_remove(&reg_path, id)?,
            }
            return Ok(());
        }
        _ => {}
    }

    if matches!(&cli.command, Commands::Init) {
        config::ensure_workspace_config_for_init(cli.config.as_deref())?;
    }
    let resolved_config = config::load_config_for_cli(cli.config.clone())?;
    let config_path = resolved_config.path.clone();
    let agent_resource_dirs = if matches!(
        &cli.command,
        Commands::Agent { .. } | Commands::Profile { .. } | Commands::Serve { .. }
    ) {
        agent_resource::cli_resource_directories(&resolved_config)?
    } else {
        vec![]
    };
    let tool_resource_dirs = if matches!(
        &cli.command,
        Commands::Tool {
            action: ToolAction::Bindings { .. }
        } | Commands::Agent {
            action: AgentAction::Run { .. }
                | AgentAction::Resume { .. }
                | AgentAction::Enqueue { .. }
                | AgentAction::Worker { .. }
        }
    ) {
        tool_binding::cli_resource_directories(&resolved_config)?
    } else {
        vec![]
    };
    let cfg = resolved_config.config;

    match cli.command {
        Commands::Init => {
            SqliteAppStore::initialize_config(&cfg).await?;
            println!("Database initialized successfully.");

            // Offer to install the community registry if not already configured
            if cfg.registries.is_empty() && atty::is(atty::Stream::Stdin) {
                eprint!("Would you like to install the community extension registry? [Y/n] ");
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_ok() {
                    let answer = input.trim().to_lowercase();
                    if answer.is_empty() || answer == "y" || answer == "yes" {
                        let registry_result = config_path
                            .as_deref()
                            .ok_or_else(|| anyhow::anyhow!("No config path available"))
                            .and_then(registry::cmd_init_community);
                        if let Err(e) = registry_result {
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
            progress,
            no_progress,
        } => {
            let progress_mode = if no_progress {
                progress::ProgressMode::Off
            } else if let Some(ref mode) = progress {
                match mode.as_str() {
                    "json" => progress::ProgressMode::Json,
                    _ => progress::ProgressMode::Human,
                }
            } else {
                progress::ProgressMode::default_for_tty()
            };
            let reporter = progress_mode.reporter();
            ingest::run_sync(
                &cfg,
                &connector,
                full,
                dry_run,
                since,
                until,
                limit,
                Some(reporter.as_ref()),
            )
            .await?;
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
        Commands::VectorIndex { action } => match action {
            VectorIndexAction::Status => {
                let status = vector_index::vector_index_status(&cfg).await?;
                println!("vector-index status");
                println!("  backend: {}", status.health.backend);
                println!("  enabled: {}", status.health.enabled);
                println!("  available: {}", status.health.available);
                println!("  path: {}", status.path.display());
                println!("  sqlite vectors: {}", status.sqlite_vector_count);
                println!("  fresh: {}", status.fresh);
                if let Some(manifest) = status.manifest {
                    println!("  sidecar vectors: {}", manifest.vector_count);
                    println!(
                        "  sidecar: metric={}, index={}, dims={}",
                        manifest.metric,
                        manifest.index,
                        manifest
                            .dims
                            .map(|dims| dims.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    );
                } else {
                    println!("  sidecar: missing");
                }
                if let Some(message) = status.health.message {
                    println!("  message: {}", message);
                }
            }
            VectorIndexAction::Rebuild => {
                let status = vector_index::rebuild_configured_vector_index(&cfg).await?;
                println!("vector-index rebuild");
                println!("  backend: {}", status.health.backend);
                println!("  path: {}", status.path.display());
                println!("  sqlite vectors: {}", status.sqlite_vector_count);
                println!("  fresh: {}", status.fresh);
            }
        },
        Commands::Serve { service } => match service {
            // The multi-workspace case (`--workspaces`) is handled earlier and
            // returns before single-config resolution; reaching here means
            // compatibility (single-workspace) mode.
            ServeService::Mcp { .. } => {
                server::run_server_with_resources(
                    &cfg,
                    std::sync::Arc::new(traits::ToolRegistry::new()),
                    std::sync::Arc::new(profiles::ProfileRegistry::new()),
                    &agent_resource_dirs,
                )
                .await?;
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
            ToolAction::Bindings { action } => {
                let catalog = tool_binding::core_metadata_catalog()?;
                let selected = match &action {
                    ToolBindingsAction::Show { name, .. } => Some(name.as_str()),
                    ToolBindingsAction::Validate {
                        name: Some(name), ..
                    } => Some(name.as_str()),
                    _ => None,
                };
                let loaded = if let Some(name) = selected {
                    tool_binding::load_named_resource(&tool_resource_dirs, &cfg, name)?
                } else {
                    tool_binding::load_resources(&tool_resource_dirs, &cfg)?
                };
                let resolved = tool_binding::resolve_resources(loaded, &catalog)?;
                match action {
                    ToolBindingsAction::List { json } => {
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&tool_binding::inspection_values(
                                    &resolved
                                )?)?
                            );
                        } else {
                            for item in resolved.values() {
                                println!(
                                    "{} — {} [{} {}]",
                                    item.binding.name,
                                    item.binding.description,
                                    item.binding.implementation_id,
                                    item.binding.implementation_version
                                );
                            }
                        }
                    }
                    ToolBindingsAction::Show { name, json } => {
                        let item = resolved
                            .get(&name)
                            .with_context(|| format!("unknown tool binding '{name}'"))?;
                        if json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&tool_binding::inspection_value(
                                    item
                                )?)?
                            );
                        } else {
                            let inspected = tool_binding::inspection_value(item)?;
                            println!("Tool: {}", item.binding.name);
                            println!("Description: {}", item.binding.description);
                            println!(
                                "Implementation description: {}",
                                item.binding.implementation_description
                            );
                            println!(
                                "Implementation: {} {}",
                                item.binding.implementation_id, item.binding.implementation_version
                            );
                            println!("Scope: {:?}", item.scope);
                            println!("Path: {}", item.path.display());
                            println!("Binding version: {}", item.binding.binding_version);
                            println!("Trust: {:?}", item.binding.trust_class);
                            println!(
                                "Capabilities: {}",
                                serde_json::to_string(&item.binding.capabilities)?
                            );
                            println!(
                                "Restrictions: {}",
                                serde_json::to_string(&item.binding.restrictions)?
                            );
                            println!(
                                "Configuration: {}",
                                serde_json::to_string(&inspected["binding"]["config"])?
                            );
                            println!(
                                "Fixed arguments: {}",
                                serde_json::to_string(&inspected["binding"]["fixed"])?
                            );
                            println!(
                                "Public schema: {}",
                                serde_json::to_string_pretty(&item.binding.public_schema)?
                            );
                        }
                    }
                    ToolBindingsAction::Validate { name, json } => {
                        if let Some(name) = name.as_deref() {
                            ensure!(resolved.contains_key(name), "unknown tool binding '{name}'");
                        }
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "valid": true,
                                    "bindings": resolved.len(),
                                    "name": name
                                })
                            );
                        } else if let Some(name) = name.as_deref() {
                            println!("Validated tool binding '{name}'.");
                        } else {
                            println!("Validated {} tool bindings.", resolved.len());
                        }
                    }
                }
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
                let path = config_path
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("No config file available for registry add"))?;
                registry::cmd_add(&cfg, &extension, path)?;
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
        // Handled above (before config loading).
        Commands::Workspace { .. } => unreachable!(),
        Commands::Agent { action } => match action {
            AgentAction::Run {
                name,
                input,
                json,
                non_interactive,
            } => {
                agent_runtime::cli::run(
                    cfg,
                    &agent_resource_dirs,
                    &tool_resource_dirs,
                    &name,
                    &input,
                    json,
                    non_interactive,
                )
                .await?;
            }
            AgentAction::Resume {
                run_id,
                json,
                non_interactive,
            } => {
                agent_runtime::cli::resume(
                    cfg,
                    &agent_resource_dirs,
                    &tool_resource_dirs,
                    &run_id,
                    json,
                    non_interactive,
                )
                .await?;
            }
            AgentAction::Enqueue {
                name,
                input,
                request_key,
                queue_limit,
                json,
            } => {
                agent_runtime::cli::enqueue(
                    cfg,
                    &agent_resource_dirs,
                    &tool_resource_dirs,
                    &name,
                    &input,
                    &request_key,
                    queue_limit,
                    json,
                )
                .await?;
            }
            AgentAction::Worker {
                worker_id,
                max_concurrency,
                lease_seconds,
                heartbeat_seconds,
                poll_ms,
                max_attempts,
                shutdown_seconds,
            } => {
                agent_runtime::cli::worker(
                    cfg,
                    &agent_resource_dirs,
                    &tool_resource_dirs,
                    worker_id,
                    max_concurrency,
                    lease_seconds,
                    heartbeat_seconds,
                    poll_ms,
                    max_attempts,
                    shutdown_seconds,
                )
                .await?;
            }
            AgentAction::Jobs { limit, json } => {
                agent_runtime::cli::jobs(cfg, limit, json).await?;
            }
            AgentAction::Job { action } => match action {
                AgentJobAction::Inspect {
                    task_id,
                    after_sequence,
                    limit,
                    json,
                } => {
                    agent_runtime::cli::job_inspect(cfg, &task_id, after_sequence, limit, json)
                        .await?;
                }
                AgentJobAction::Cancel { task_id, json } => {
                    agent_runtime::cli::job_cancel(cfg, &task_id, json).await?;
                }
            },
            AgentAction::History { limit, json } => {
                agent_runtime::cli::history(cfg, limit, json).await?;
            }
            AgentAction::Inspect {
                run_id,
                after_sequence,
                limit,
                json,
            } => {
                agent_runtime::cli::inspect(cfg, &run_id, after_sequence, limit, json).await?;
            }
            AgentAction::List { json } => {
                agent_resource::list(&cfg, &agent_resource_dirs, json)?;
            }
            AgentAction::Show { name, json } => {
                agent_resource::show(&cfg, &agent_resource_dirs, &name, json)?;
            }
            AgentAction::Validate => {
                agent_resource::validate(&cfg, &agent_resource_dirs)?;
            }
            AgentAction::Test { name, args } => {
                eprintln!("warning: `ctx agent test` is deprecated; use `ctx profile test`");
                agent_resource::test_profile(&cfg, &agent_resource_dirs, &name, args).await?;
            }
            AgentAction::Init { .. } => {
                // Handled above (before config loading)
                unreachable!()
            }
        },
        Commands::Profile { action } => match action {
            ProfileAction::List { json } => {
                agent_resource::list_profiles(&cfg, &agent_resource_dirs, json)?;
            }
            ProfileAction::Show { name, json } => {
                agent_resource::show_profile(&cfg, &agent_resource_dirs, &name, json)?;
            }
            ProfileAction::Validate => {
                agent_resource::validate_profiles(&cfg, &agent_resource_dirs)?;
            }
            ProfileAction::Test { name, args } => {
                agent_resource::test_profile(&cfg, &agent_resource_dirs, &name, args).await?;
            }
            ProfileAction::Init { .. } => unreachable!(),
        },
    }

    Ok(())
}
