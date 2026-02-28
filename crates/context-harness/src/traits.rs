//! Extension traits for custom connectors and tools.
//!
//! This module provides the trait-based extension system for Context Harness.
//! Users can implement [`Connector`] and [`Tool`] in Rust to create compiled
//! extensions that run alongside built-in and Lua-scripted ones.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │           ConnectorRegistry              │
//! │  ┌─────────┐ ┌─────────┐ ┌────────────┐ │
//! │  │Built-in │ │  Lua    │ │  Custom    │ │
//! │  │FS/Git/S3│ │ Script  │ │ (Rust)     │ │
//! │  └─────────┘ └─────────┘ └────────────┘ │
//! └──────────────┬───────────────────────────┘
//!                ▼
//!          run_sync() → ingest pipeline
//! ```
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │              ToolRegistry                │
//! │  ┌─────────┐ ┌─────────┐ ┌────────────┐ │
//! │  │Built-in │ │  Lua    │ │  Custom    │ │
//! │  │search   │ │ Script  │ │ (Rust)     │ │
//! │  │get/src  │ │ Tools   │ │  Tools     │ │
//! │  └─────────┘ └─────────┘ └────────────┘ │
//! └──────────────┬───────────────────────────┘
//!                ▼
//!          run_server() → MCP HTTP API
//! ```
//!
//! # Usage
//!
//! ```rust
//! use context_harness::traits::{ConnectorRegistry, ToolRegistry};
//!
//! let mut connectors = ConnectorRegistry::new();
//! // connectors.register(Box::new(MyConnector::new()));
//!
//! let mut tools = ToolRegistry::new();
//! // tools.register(Box::new(MyTool::new()));
//! ```
//!
//! See `docs/RUST_TRAITS.md` for the full specification and examples.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::config::Config;
use crate::get::{get_document, DocumentResponse};
use crate::models::SourceItem;
use crate::search::{search_documents, SearchResultItem};
use crate::sources::{get_sources, SourceStatus};

// ═══════════════════════════════════════════════════════════════════════
// Connector Trait
// ═══════════════════════════════════════════════════════════════════════

/// A data source connector that produces documents for ingestion.
///
/// Implement this trait to create a custom connector in Rust. The
/// connector is responsible for scanning an external data source and
/// returning a list of [`SourceItem`]s that flow through the standard
/// ingestion pipeline (normalization → chunking → embedding).
///
/// # Lifecycle
///
/// 1. The connector is registered via [`ConnectorRegistry::register`].
/// 2. [`scan`](Connector::scan) is called during `ctx sync custom:<name>`.
/// 3. Returned items are normalized, chunked, and indexed.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use anyhow::Result;
/// use context_harness::models::SourceItem;
/// use context_harness::traits::Connector;
/// use chrono::Utc;
///
/// pub struct DatabaseConnector {
///     connection_string: String,
/// }
///
/// #[async_trait]
/// impl Connector for DatabaseConnector {
///     fn name(&self) -> &str { "database" }
///     fn description(&self) -> &str { "Ingest rows from a database table" }
///     fn connector_type(&self) -> &str { "custom" }
///
///     async fn scan(&self) -> Result<Vec<SourceItem>> {
///         // ... query database and return SourceItems
///         Ok(vec![])
///     }
/// }
/// ```
#[async_trait]
pub trait Connector: Send + Sync {
    /// Returns the connector instance name (e.g. `"docs"`, `"platform"`).
    ///
    /// Combined with [`connector_type`](Connector::connector_type) to form
    /// the source label: `"{type}:{name}"`.
    fn name(&self) -> &str;

    /// Returns a one-line description of what this connector does.
    ///
    /// Used in `ctx sources` output and documentation.
    fn description(&self) -> &str;

    /// Returns the connector type identifier (e.g. `"filesystem"`, `"git"`, `"s3"`, `"custom"`).
    ///
    /// Built-in connectors return their type name; custom (user-defined)
    /// connectors default to `"custom"`.
    fn connector_type(&self) -> &str {
        "custom"
    }

    /// Returns the source label used to tag documents from this connector.
    ///
    /// Defaults to `"{connector_type}:{name}"` (e.g. `"git:platform"`).
    fn source_label(&self) -> String {
        format!("{}:{}", self.connector_type(), self.name())
    }

    /// Scan the data source and return all items to ingest.
    ///
    /// Called on the tokio async runtime. May perform I/O operations
    /// (HTTP requests, database queries, file reads).
    ///
    /// # Returns
    ///
    /// A vector of [`SourceItem`]s. Each item flows through the standard
    /// ingestion pipeline. Items with empty `body` or `source_id` are
    /// skipped with a warning.
    async fn scan(&self) -> Result<Vec<SourceItem>>;
}

// ═══════════════════════════════════════════════════════════════════════
// Tool Trait
// ═══════════════════════════════════════════════════════════════════════

/// A custom MCP tool that agents can discover and call.
///
/// Implement this trait to create a compiled Rust tool. Tools are
/// registered at server startup and exposed via `GET /tools/list`
/// for agent discovery and `POST /tools/{name}` for invocation.
///
/// # Lifecycle
///
/// 1. The tool is registered via [`ToolRegistry::register`].
/// 2. [`name`](Tool::name), [`description`](Tool::description), and
///    [`parameters_schema`](Tool::parameters_schema) are called at startup
///    for the tool list.
/// 3. [`execute`](Tool::execute) is called each time an agent invokes
///    the tool.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use anyhow::Result;
/// use serde_json::{json, Value};
/// use context_harness::traits::{Tool, ToolContext};
///
/// pub struct HealthCheckTool;
///
/// #[async_trait]
/// impl Tool for HealthCheckTool {
///     fn name(&self) -> &str { "health_check" }
///     fn description(&self) -> &str { "Check connector health" }
///
///     fn parameters_schema(&self) -> Value {
///         json!({
///             "type": "object",
///             "properties": {},
///             "required": []
///         })
///     }
///
///     async fn execute(&self, _params: Value, ctx: &ToolContext) -> Result<Value> {
///         let sources = ctx.sources()?;
///         Ok(json!({ "sources": sources.len() }))
///     }
/// }
/// ```
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the tool's name.
    ///
    /// Used as the route path (`POST /tools/{name}`) and in
    /// `GET /tools/list` responses. Should be a lowercase
    /// identifier with underscores (e.g., `"create_ticket"`).
    fn name(&self) -> &str;

    /// Returns a one-line description for agent discovery.
    ///
    /// Agents use this to decide whether to call the tool.
    fn description(&self) -> &str;

    /// Whether this tool is a built-in (true for search/get/sources).
    ///
    /// Built-in tools are marked with `"builtin": true` in the
    /// `GET /tools/list` response. Defaults to `false`.
    fn is_builtin(&self) -> bool {
        false
    }

    /// Returns the OpenAI function-calling JSON Schema for parameters.
    ///
    /// Must be a valid JSON Schema object with `type: "object"`,
    /// `properties`, and optionally `required`.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with validated parameters.
    ///
    /// Called each time an agent invokes the tool via `POST /tools/{name}`.
    ///
    /// # Arguments
    ///
    /// * `params` — JSON parameters (always a JSON object).
    /// * `ctx` — Bridge to the Context Harness knowledge base.
    ///
    /// # Returns
    ///
    /// A JSON value that will be wrapped in `{ "result": ... }` in the
    /// HTTP response.
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value>;
}

// ═══════════════════════════════════════════════════════════════════════
// ToolContext
// ═══════════════════════════════════════════════════════════════════════

/// Options for [`ToolContext::search`].
#[derive(Debug, Default)]
pub struct SearchOptions {
    /// Search mode: `"keyword"`, `"semantic"`, or `"hybrid"`.
    pub mode: Option<String>,
    /// Maximum number of results.
    pub limit: Option<i64>,
    /// Filter by source connector (e.g., `"git:platform"`).
    pub source: Option<String>,
}

/// Context bridge for tool execution.
///
/// Provides tools with access to the Context Harness knowledge base
/// during execution. Created by the server for each tool invocation.
///
/// All methods delegate to the same core functions used by the CLI
/// and HTTP server, ensuring tools have identical capabilities.
pub struct ToolContext {
    config: Arc<Config>,
}

impl ToolContext {
    /// Create a new tool context from the application config.
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Search the knowledge base.
    ///
    /// Equivalent to `POST /tools/search` or `ctx search`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use context_harness::traits::{ToolContext, SearchOptions};
    /// # async fn example(ctx: &ToolContext) -> anyhow::Result<()> {
    /// let results = ctx.search("deployment runbook", SearchOptions {
    ///     mode: Some("hybrid".to_string()),
    ///     limit: Some(5),
    ///     ..Default::default()
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn search(&self, query: &str, opts: SearchOptions) -> Result<Vec<SearchResultItem>> {
        search_documents(
            &self.config,
            query,
            opts.mode.as_deref().unwrap_or("keyword"),
            opts.source.as_deref(),
            None,
            opts.limit,
            false,
        )
        .await
    }

    /// Retrieve a document by UUID.
    ///
    /// Equivalent to `POST /tools/get` or `ctx get`.
    pub async fn get(&self, id: &str) -> Result<DocumentResponse> {
        get_document(&self.config, id).await
    }

    /// List all configured connectors and their status.
    ///
    /// Equivalent to `GET /tools/sources` or `ctx sources`.
    pub fn sources(&self) -> Result<Vec<SourceStatus>> {
        Ok(get_sources(&self.config))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Built-in Tool Implementations
// ═══════════════════════════════════════════════════════════════════════

/// Built-in search tool. Delegates to [`ToolContext::search`].
pub struct SearchTool;

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search the knowledge base"
    }

    fn is_builtin(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"], "default": "keyword" },
                "limit": { "type": "integer", "description": "Max results", "default": 12 },
                "filters": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Filter by connector source" },
                        "since": { "type": "string", "description": "Only results updated after this date (YYYY-MM-DD)" }
                    }
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let query = params["query"].as_str().unwrap_or("");
        if query.trim().is_empty() {
            anyhow::bail!("query must not be empty");
        }

        let mode = params["mode"].as_str().unwrap_or("keyword");
        let limit = params["limit"].as_i64().unwrap_or(12);

        let source = params
            .get("filters")
            .and_then(|f| f.get("source"))
            .and_then(|s| s.as_str());
        let since = params
            .get("filters")
            .and_then(|f| f.get("since"))
            .and_then(|s| s.as_str());

        let results =
            search_documents(&ctx.config, query, mode, source, since, Some(limit), false).await?;

        Ok(serde_json::json!({ "results": results }))
    }
}

/// Built-in document retrieval tool. Delegates to [`get_document`].
pub struct GetTool;

#[async_trait]
impl Tool for GetTool {
    fn name(&self) -> &str {
        "get"
    }

    fn description(&self) -> &str {
        "Retrieve a document by UUID"
    }

    fn is_builtin(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Document UUID" }
            },
            "required": ["id"]
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let id = params["id"].as_str().unwrap_or("");
        if id.trim().is_empty() {
            anyhow::bail!("id must not be empty");
        }

        let doc = get_document(&ctx.config, id).await?;
        Ok(serde_json::to_value(&doc)?)
    }
}

/// Built-in sources listing tool. Delegates to [`get_sources`].
pub struct SourcesTool;

#[async_trait]
impl Tool for SourcesTool {
    fn name(&self) -> &str {
        "sources"
    }

    fn description(&self) -> &str {
        "List connector configuration and health status"
    }

    fn is_builtin(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _params: Value, ctx: &ToolContext) -> Result<Value> {
        let sources = get_sources(&ctx.config);
        Ok(serde_json::json!({ "sources": sources }))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Registries
// ═══════════════════════════════════════════════════════════════════════

/// Registry for connectors (built-in and custom).
///
/// Use [`ConnectorRegistry::from_config`] to create a registry pre-loaded
/// with all built-in connectors from the config file, then optionally
/// call [`register`](ConnectorRegistry::register) to add custom ones.
///
/// # Example
///
/// ```rust
/// use context_harness::traits::ConnectorRegistry;
///
/// let mut connectors = ConnectorRegistry::new();
/// // connectors.register(Box::new(MyConnector::new()));
/// ```
pub struct ConnectorRegistry {
    connectors: Vec<Box<dyn Connector>>,
}

impl ConnectorRegistry {
    /// Create an empty connector registry.
    pub fn new() -> Self {
        Self {
            connectors: Vec::new(),
        }
    }

    /// Create a registry pre-loaded with all built-in connectors from the config.
    ///
    /// This resolves all filesystem, git, S3, and script connector instances
    /// from the TOML config and wraps them as trait objects.
    pub fn from_config(config: &Config) -> Self {
        use crate::connector_fs::FilesystemConnector;
        use crate::connector_git::GitConnector;
        use crate::connector_s3::S3Connector;
        use crate::connector_script::ScriptConnector;

        let mut registry = Self::new();

        for (name, cfg) in &config.connectors.filesystem {
            registry.register(Box::new(FilesystemConnector::new(
                name.clone(),
                cfg.clone(),
            )));
        }
        for (name, cfg) in &config.connectors.git {
            registry.register(Box::new(GitConnector::new(
                name.clone(),
                cfg.clone(),
                config.db.path.clone(),
            )));
        }
        for (name, cfg) in &config.connectors.s3 {
            registry.register(Box::new(S3Connector::new(name.clone(), cfg.clone())));
        }
        for (name, cfg) in &config.connectors.script {
            registry.register(Box::new(ScriptConnector::new(name.clone(), cfg.clone())));
        }

        registry
    }

    /// Register a connector.
    pub fn register(&mut self, connector: Box<dyn Connector>) {
        self.connectors.push(connector);
    }

    /// Get all registered connectors.
    pub fn connectors(&self) -> &[Box<dyn Connector>] {
        &self.connectors
    }

    /// Get connectors filtered by type (e.g. `"git"`, `"filesystem"`).
    pub fn connectors_by_type(&self, connector_type: &str) -> Vec<&dyn Connector> {
        self.connectors
            .iter()
            .filter(|c| c.connector_type() == connector_type)
            .map(|c| c.as_ref())
            .collect()
    }

    /// Find a specific connector by type and name.
    pub fn find(&self, connector_type: &str, name: &str) -> Option<&dyn Connector> {
        self.connectors
            .iter()
            .find(|c| c.connector_type() == connector_type && c.name() == name)
            .map(|c| c.as_ref())
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.connectors.is_empty()
    }

    /// Return the count of registered connectors.
    pub fn len(&self) -> usize {
        self.connectors.len()
    }
}

impl Default for ConnectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry for tools (built-in, Lua, and custom Rust).
///
/// Use [`ToolRegistry::with_builtins`] to create a registry pre-loaded
/// with the core `search`, `get`, and `sources` tools, then optionally
/// call [`register`](ToolRegistry::register) to add custom ones.
///
/// # Example
///
/// ```rust
/// use context_harness::traits::ToolRegistry;
///
/// let mut tools = ToolRegistry::with_builtins();
/// // tools.register(Box::new(MyTool::new()));
/// ```
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty tool registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Create a tool registry pre-loaded with built-in tools (search, get, sources).
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(SearchTool));
        registry.register(Box::new(GetTool));
        registry.register(Box::new(SourcesTool));
        registry
    }

    /// Register a tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Get all registered tools.
    pub fn tools(&self) -> &[Box<dyn Tool>] {
        &self.tools
    }

    /// Find a tool by name.
    pub fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Return the count of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
