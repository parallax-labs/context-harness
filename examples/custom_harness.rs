//! Example: Custom Context Harness binary with Rust trait extensions.
//!
//! Demonstrates building a custom binary that extends Context Harness with:
//! - A **`JsonConnector`** that reads documents from a JSON file
//! - A **`StatsTool`** that queries the knowledge base and returns statistics
//! - A **`RunbookAgent`** that pre-searches for runbooks and injects them as context
//!
//! # Running
//!
//! ```bash
//! # 1. Create a JSON document source
//! cat > /tmp/docs.json << 'EOF'
//! [
//!   {
//!     "id": "runbook-deploy",
//!     "title": "Deployment Runbook",
//!     "body": "Step 1: Pull latest from main.\nStep 2: Run cargo build --release.\nStep 3: Deploy via systemd.",
//!     "author": "ops-team"
//!   },
//!   {
//!     "id": "runbook-rollback",
//!     "title": "Rollback Procedure",
//!     "body": "If deployment fails, revert to the previous release tag.\nUse: git checkout v1.2.3 && cargo build --release.",
//!     "author": "ops-team"
//!   },
//!   {
//!     "id": "adr-001",
//!     "title": "ADR-001: Use SQLite for Local Storage",
//!     "body": "We chose SQLite because it is serverless, zero-config, and supports FTS5 for full-text search.\nAlternatives considered: PostgreSQL (too heavy), DuckDB (no FTS5).",
//!     "author": "architecture"
//!   }
//! ]
//! EOF
//!
//! # 2. Create a config file
//! mkdir -p /tmp/custom-harness/config
//! cat > /tmp/custom-harness/config/ctx.toml << 'EOF'
//! [db]
//! path = "/tmp/custom-harness/data/ctx.sqlite"
//!
//! [chunking]
//! max_tokens = 700
//!
//! [server]
//! bind = "127.0.0.1:7480"
//! EOF
//!
//! # 3. Run sync (init + ingest from JSON file)
//! cargo run --example custom_harness -- \
//!   --config /tmp/custom-harness/config/ctx.toml \
//!   sync /tmp/docs.json
//!
//! # 4. Start the server with the stats tool
//! cargo run --example custom_harness -- \
//!   --config /tmp/custom-harness/config/ctx.toml \
//!   serve
//!
//! # 5. In another terminal, query the stats tool and agents
//! curl -s http://localhost:7480/tools/list | jq .
//! curl -s -X POST http://localhost:7480/tools/kb_stats \
//!   -H 'Content-Type: application/json' \
//!   -d '{"query": "deployment"}' | jq .
//!
//! # 6. List agents and resolve the runbook-expert agent
//! curl -s http://localhost:7480/agents/list | jq .
//! curl -s -X POST http://localhost:7480/agents/runbook-expert/prompt \
//!   -H 'Content-Type: application/json' \
//!   -d '{"topic": "deployment"}' | jq .
//! ```

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use context_harness::agents::{Agent, AgentPrompt, AgentRegistry};
use context_harness::config;
use context_harness::ingest::run_sync_with_extensions;
use context_harness::migrate;
use context_harness::models::SourceItem;
use context_harness::server::run_server_with_extensions;
use context_harness::traits::{
    Connector, ConnectorRegistry, SearchOptions, Tool, ToolContext, ToolRegistry,
};

// ═══════════════════════════════════════════════════════════════════════
// JSON File Connector
// ═══════════════════════════════════════════════════════════════════════

/// A connector that reads documents from a JSON file.
///
/// Each entry in the JSON array becomes a [`SourceItem`]. This demonstrates
/// how to implement the [`Connector`] trait for a custom data source.
struct JsonConnector {
    /// Path to the JSON file containing documents.
    file_path: PathBuf,
}

/// Shape of each document in the JSON file.
#[derive(Deserialize)]
struct JsonDoc {
    id: String,
    title: String,
    body: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

impl JsonConnector {
    fn new(file_path: PathBuf) -> Self {
        Self { file_path }
    }
}

#[async_trait]
impl Connector for JsonConnector {
    fn name(&self) -> &str {
        "json_file"
    }

    fn description(&self) -> &str {
        "Ingest documents from a JSON file"
    }

    async fn scan(&self) -> Result<Vec<SourceItem>> {
        let content = std::fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read JSON file: {}", self.file_path.display()))?;

        let docs: Vec<JsonDoc> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON file: {}", self.file_path.display()))?;

        let now = Utc::now();

        let items: Vec<SourceItem> = docs
            .into_iter()
            .map(|doc| SourceItem {
                source: "custom:json_file".to_string(),
                source_id: doc.id,
                source_url: doc.url,
                title: Some(doc.title),
                author: doc.author,
                created_at: now,
                updated_at: now,
                content_type: "text/plain".to_string(),
                body: doc.body,
                metadata_json: "{}".to_string(),
                raw_json: None,
            })
            .collect();

        println!("  JsonConnector: read {} documents", items.len());
        Ok(items)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Knowledge Base Stats Tool
// ═══════════════════════════════════════════════════════════════════════

/// A tool that returns statistics about the knowledge base.
///
/// Agents can call this to understand what's indexed, search for
/// relevant content, and get summary info. This demonstrates
/// how to implement the [`Tool`] trait with [`ToolContext`] access.
struct StatsTool;

#[async_trait]
impl Tool for StatsTool {
    fn name(&self) -> &str {
        "kb_stats"
    }

    fn description(&self) -> &str {
        "Search the knowledge base and return statistics about matching documents"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant documents"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let query = params["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("query parameter is required"))?;
        let limit = params["limit"].as_i64().unwrap_or(10);

        // Use the ToolContext to search the knowledge base
        let results = ctx
            .search(
                query,
                SearchOptions {
                    mode: Some("keyword".to_string()),
                    limit: Some(limit),
                    ..Default::default()
                },
            )
            .await?;

        // List all sources
        let sources = ctx.sources()?;
        let configured_sources: Vec<&str> = sources
            .iter()
            .filter(|s| s.configured)
            .map(|s| s.name.as_str())
            .collect();

        // Build summary
        let result_summaries: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "title": r.title,
                    "score": r.score,
                    "source": r.source,
                    "snippet_length": r.snippet.len(),
                })
            })
            .collect();

        Ok(json!({
            "query": query,
            "total_results": results.len(),
            "results": result_summaries,
            "configured_sources": configured_sources,
            "source_count": sources.len(),
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Runbook Agent
// ═══════════════════════════════════════════════════════════════════════

/// A custom agent that pre-searches for runbooks and injects them into
/// the system prompt. This demonstrates how to implement the [`Agent`]
/// trait with dynamic context injection via [`ToolContext`].
struct RunbookAgent;

#[async_trait]
impl Agent for RunbookAgent {
    fn name(&self) -> &str {
        "runbook-expert"
    }

    fn description(&self) -> &str {
        "Helps follow operational runbooks and procedures"
    }

    fn tools(&self) -> Vec<String> {
        vec!["search".into(), "get".into(), "kb_stats".into()]
    }

    fn source(&self) -> &str {
        "rust"
    }

    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<AgentPrompt> {
        let topic = args["topic"].as_str().unwrap_or("operations");

        // Pre-search for relevant runbooks to inject as context
        let results = ctx
            .search(
                topic,
                SearchOptions {
                    mode: Some("keyword".to_string()),
                    limit: Some(3),
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_default();

        // Build context snippets from search results
        let mut context_text = String::new();
        for r in &results {
            if let Some(ref title) = r.title {
                context_text.push_str(&format!("\n## {}\n", title));
            }
            context_text.push_str(&r.snippet);
            context_text.push('\n');
        }

        let system = format!(
            r#"You are a runbook expert focused on {topic}.

You have access to the knowledge base and should use the search and get tools
to find relevant operational procedures.

Here are some relevant documents I found for you:
{context_text}

When answering:
1. Reference specific runbook steps
2. Suggest related procedures the user should review
3. Warn about common pitfalls"#,
        );

        Ok(AgentPrompt {
            system,
            tools: self.tools(),
            messages: vec![],
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// CLI
// ═══════════════════════════════════════════════════════════════════════

/// Custom Context Harness binary with JSON connector and stats tool.
#[derive(Parser)]
#[command(
    name = "custom-harness",
    about = "Custom Context Harness with extensions"
)]
struct Cli {
    /// Path to configuration file (TOML).
    #[arg(long, default_value = "./config/ctx.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync documents from a JSON file into the knowledge base.
    Sync {
        /// Path to the JSON file containing documents.
        json_file: PathBuf,
    },
    /// Start the MCP server with the stats tool.
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::load_config(&cli.config)?;

    match cli.command {
        Commands::Sync { json_file } => {
            if !json_file.exists() {
                bail!("JSON file not found: {}", json_file.display());
            }

            // Initialize the database
            migrate::run_migrations(&cfg).await?;
            println!("Database initialized.");

            // Register our custom connector
            let mut connectors = ConnectorRegistry::new();
            connectors.register(Box::new(JsonConnector::new(json_file)));

            // Sync using the trait-based pipeline
            run_sync_with_extensions(
                &cfg,
                "custom:json_file",
                true, // full sync
                false,
                None,
                None,
                None,
                &connectors,
            )
            .await?;

            println!("Sync complete.");
        }
        Commands::Serve => {
            // Register our custom tool
            let mut tools = ToolRegistry::new();
            tools.register(Box::new(StatsTool));

            // Register our custom agent
            let mut agents = AgentRegistry::new();
            agents.register(Box::new(RunbookAgent));

            println!("Starting server with custom StatsTool + RunbookAgent...");
            run_server_with_extensions(&cfg, Arc::new(tools), Arc::new(agents)).await?;
        }
    }

    Ok(())
}
