+++
title = "Rust Traits"
description = "Extend Context Harness with custom connectors, tools, and agents using compiled Rust traits."
weight = 4
+++

For maximum performance and type safety, implement custom connectors, tools, and agents as compiled Rust code using the extension traits. This is the same interface used internally by all built-in components.

### When to use Rust traits vs. Lua

| Approach | Best for |
|----------|----------|
| **Rust traits** | High-performance connectors, type-safe tools, compiled binaries, internal APIs |
| **Lua scripts** | Quick prototyping, runtime extensibility, LLM-generated connectors, API integrations |

### Building a custom binary

The extension traits are designed for building custom Context Harness binaries. Create a new Rust project that depends on `context-harness`:

```toml
# Cargo.toml
[dependencies]
context-harness = { git = "https://github.com/parallax-labs/context-harness" }
async-trait = "0.1"
anyhow = "1"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
```

---

### Custom connector

Implement the `Connector` trait to add a new data source:

```rust
use context_harness::traits::Connector;
use context_harness::models::SourceItem;
use async_trait::async_trait;
use anyhow::Result;
use std::sync::Arc;

pub struct ApiDocsConnector {
    api_url: String,
}

#[async_trait]
impl Connector for ApiDocsConnector {
    fn name(&self) -> &str { "api-docs" }
    fn source_label(&self) -> &str { "custom:api-docs" }

    async fn scan(&self) -> Result<Vec<SourceItem>> {
        let resp = reqwest::get(&format!("{}/docs", self.api_url))
            .await?
            .json::<Vec<ApiDoc>>()
            .await?;

        Ok(resp.into_iter().map(|doc| SourceItem {
            source: self.source_label().to_string(),
            source_id: doc.id.clone(),
            source_url: Some(format!("{}/docs/{}", self.api_url, doc.id)),
            title: Some(doc.title),
            body: doc.content,
            updated_at: doc.updated_at,
        }).collect())
    }
}
```

Register and sync:

```rust
use context_harness::traits::ConnectorRegistry;

let mut connectors = ConnectorRegistry::new();
connectors.register(Box::new(ApiDocsConnector {
    api_url: "https://api.internal.example.com".into(),
}));

// Sync with the database
context_harness::run_sync_with_extensions(&config, &connectors, "custom:api-docs").await?;
```

---

### Custom tool

Implement the `Tool` trait to add a new MCP tool:

```rust
use context_harness::traits::{Tool, ToolContext};
use async_trait::async_trait;
use anyhow::Result;
use serde_json::{json, Value};

pub struct RunQueryTool;

#[async_trait]
impl Tool for RunQueryTool {
    fn name(&self) -> &str { "run_query" }

    fn description(&self) -> &str {
        "Execute a read-only SQL query against the analytics database"
    }

    fn is_builtin(&self) -> bool { false }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "Read-only SQL query"
                },
                "database": {
                    "type": "string",
                    "description": "Target database name"
                }
            },
            "required": ["sql"]
        })
    }

    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<Value> {
        let sql = params["sql"].as_str()
            .ok_or_else(|| anyhow::anyhow!("missing 'sql' parameter"))?;

        // Validate read-only
        let lower = sql.to_lowercase();
        if lower.contains("insert") || lower.contains("update") || lower.contains("delete") {
            anyhow::bail!("only SELECT queries are allowed");
        }

        // Execute against your database
        let results = execute_analytics_query(sql).await?;
        Ok(json!({ "rows": results, "query": sql }))
    }
}
```

Tools registered via `ToolRegistry` automatically appear in `GET /tools/list` and are callable via `POST /tools/{name}`.

---

### Custom agent

Implement the `Agent` trait for compiled agents:

```rust
use context_harness::{Agent, AgentPrompt, AgentArgument};
use context_harness::traits::ToolContext;
use async_trait::async_trait;

pub struct DatabaseExpert;

#[async_trait]
impl Agent for DatabaseExpert {
    fn name(&self) -> &str { "db-expert" }
    fn description(&self) -> &str { "Database design and query optimization" }
    fn tools(&self) -> Vec<String> { vec!["search".into(), "get".into(), "run_query".into()] }

    fn arguments(&self) -> Vec<AgentArgument> {
        vec![AgentArgument {
            name: "database".into(),
            description: "Target database".into(),
            required: false,
        }]
    }

    async fn resolve(
        &self, args: serde_json::Value, ctx: &ToolContext,
    ) -> anyhow::Result<AgentPrompt> {
        let db = args["database"].as_str().unwrap_or("analytics");

        // Pre-fetch relevant schema docs
        let results = ctx.search("database schema", None, None, None).await?;
        let context: String = results.iter()
            .map(|r| format!("- {}", r.title.as_deref().unwrap_or("?")))
            .collect::<Vec<_>>().join("\n");

        Ok(AgentPrompt {
            system: format!(
                "You are a database expert for '{}'.\n\nRelevant docs:\n{}\n\n\
                 Use search to find more context. Use run_query for read-only queries.",
                db, context
            ),
            tools: self.tools(),
            messages: vec![],
        })
    }
}
```

---

### Putting it all together

Here's a complete custom binary with all three extension types:

```rust
use context_harness::config::Config;
use context_harness::server::run_server_with_extensions;
use context_harness::traits::{ConnectorRegistry, ToolRegistry};
use context_harness::agents::AgentRegistry;
use std::sync::Arc;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "./config/ctx.toml")]
    config: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    Serve,
    Sync,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;

    // Register custom extensions
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(RunQueryTool));

    let mut agents = AgentRegistry::new();
    agents.register(Box::new(DatabaseExpert));

    let mut connectors = ConnectorRegistry::new();
    connectors.register(Box::new(ApiDocsConnector { /* ... */ }));

    match cli.command {
        Commands::Sync => {
            context_harness::run_sync_with_extensions(
                &config, &connectors, "all"
            ).await?;
        }
        Commands::Serve => {
            run_server_with_extensions(
                &config,
                Arc::new(tools),
                Arc::new(agents),
            ).await?;
        }
    }

    Ok(())
}
```

See the full working example at [`examples/custom_harness.rs`](https://github.com/parallax-labs/context-harness/blob/main/examples/custom_harness.rs).

---

### ToolContext bridge

Custom tools and agents can access the knowledge base via `ToolContext`:

```rust
async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
    // Search the knowledge base
    let results = ctx.search("auth flow", Some("hybrid"), Some(5), None).await?;

    // Get a full document
    let doc = ctx.get(&results[0].id).await?;

    // List all sources
    let sources = ctx.sources().await?;

    Ok(json!({ "found": results.len() }))
}
```

| Method | Description |
|--------|-------------|
| `ctx.search(query, mode?, limit?, source?)` | Search with keyword/semantic/hybrid |
| `ctx.get(id)` | Retrieve full document by UUID |
| `ctx.sources()` | List all data sources |

---

### What's next?

- [MCP Agents](/docs/guides/agents/) — define agents in TOML, Lua, or Rust
- [Lua Tools](/docs/connectors/lua-tools/) — Lua-based tools for rapid prototyping
- [Deployment](/docs/reference/deployment/) — deploy custom binaries in Docker


