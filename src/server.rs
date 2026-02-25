//! MCP-compatible HTTP server.
//!
//! Exposes Context Harness functionality via a JSON HTTP API suitable for
//! integration with Cursor, Claude, and other MCP-compatible AI tools.
//!
//! All tools — built-in (search, get, sources), Lua scripts, and custom Rust
//! trait implementations — are registered in a unified [`ToolRegistry`] and
//! dispatched through the same `POST /tools/{name}` handler.
//!
//! Agents (named personas with system prompts and tool scoping) are registered
//! in an [`AgentRegistry`] and discoverable/resolvable via dedicated endpoints.
//!
//! # Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `GET`  | `/tools/list` | List all registered tools with schemas |
//! | `POST` | `/tools/{name}` | Call any registered tool by name |
//! | `GET`  | `/agents/list` | List all registered agents with metadata |
//! | `POST` | `/agents/{name}/prompt` | Resolve an agent's system prompt |
//! | `GET`  | `/health` | Health check (returns version) |
//!
//! # Error Contract
//!
//! All error responses follow the schema defined in `docs/SCHEMAS.md`:
//!
//! ```json
//! { "error": { "code": "bad_request", "message": "query must not be empty" } }
//! ```
//!
//! Error codes: `bad_request` (400), `not_found` (404), `embeddings_disabled` (400),
//! `timeout` (408), `tool_error` (500), `internal` (500).
//!
//! # CORS
//!
//! All origins, methods, and headers are permitted to support browser-based
//! clients and cross-origin MCP tool calls.
//!
//! # Cursor Integration
//!
//! Start the server and point Cursor at the `/mcp` endpoint:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "context-harness": {
//!       "url": "http://127.0.0.1:7331/mcp"
//!     }
//!   }
//! }
//! ```

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpService,
};
use serde::Serialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::agent_script::{load_agent_definitions, LuaAgentAdapter};
use crate::agents::{AgentInfo, AgentRegistry};
use crate::config::Config;
use crate::mcp::McpBridge;
use crate::registry::RegistryManager;
use crate::tool_script::{load_tool_definitions, validate_params, LuaToolAdapter, ToolInfo};
use crate::traits::{ToolContext, ToolRegistry};

/// Shared application state passed to all route handlers via Axum's `State` extractor.
#[derive(Clone)]
struct AppState {
    /// Application configuration (wrapped in `Arc` for cheap cloning across handlers).
    config: Arc<Config>,
    /// Unified tool registry containing built-in, Lua, and custom Rust tools.
    tools: Arc<ToolRegistry>,
    /// Agent registry containing TOML, Lua, and custom Rust agents.
    agents: Arc<AgentRegistry>,
}

/// Extra extensions (custom Rust tools and agents) passed alongside the main `AppState`.
type ExtState = (Arc<ToolRegistry>, Arc<AgentRegistry>);

/// Starts the MCP-compatible HTTP server.
///
/// Binds to the address configured in `[server].bind` and registers all
/// route handlers. The server runs indefinitely until the process is terminated.
///
/// This is the standard entry point used by the `ctx serve mcp` command.
/// For custom binaries with Rust extensions, use
/// [`run_server_with_extensions`] instead.
///
/// # Arguments
///
/// - `config` — application configuration (database path, retrieval settings, bind address).
///
/// # Returns
///
/// Returns `Ok(())` when the server shuts down, or an error if binding fails.
pub async fn run_server(config: &Config) -> anyhow::Result<()> {
    run_server_with_extensions(
        config,
        Arc::new(ToolRegistry::new()),
        Arc::new(AgentRegistry::new()),
    )
    .await
}

/// Starts the MCP server with custom Rust tool and agent extensions.
///
/// Like [`run_server`], but accepts a [`ToolRegistry`] and [`AgentRegistry`]
/// containing custom extensions that will be served alongside built-in,
/// TOML-defined, and Lua-scripted entries.
///
/// Custom tools appear in `GET /tools/list` and can be called via
/// `POST /tools/{name}`. Custom agents appear in `GET /agents/list` and
/// can be resolved via `POST /agents/{name}/prompt`.
///
/// # Example
///
/// ```rust,no_run
/// use context_harness::server::run_server_with_extensions;
/// use context_harness::traits::ToolRegistry;
/// use context_harness::agents::AgentRegistry;
/// use std::sync::Arc;
///
/// # async fn example(config: &context_harness::config::Config) -> anyhow::Result<()> {
/// let tools = ToolRegistry::new();
/// let agents = AgentRegistry::new();
/// run_server_with_extensions(config, Arc::new(tools), Arc::new(agents)).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_server_with_extensions(
    config: &Config,
    extra_tools: Arc<ToolRegistry>,
    extra_agents: Arc<AgentRegistry>,
) -> anyhow::Result<()> {
    let bind_addr = config.server.bind.clone();
    let config = Arc::new(config.clone());

    // ── Tools ──
    let mut tool_registry = ToolRegistry::with_builtins();

    // Load and register Lua tools from config
    let lua_defs = load_tool_definitions(&config)?;
    let configured_tool_names: Vec<String> = lua_defs.iter().map(|d| d.name.clone()).collect();
    for def in lua_defs {
        tool_registry.register(Box::new(LuaToolAdapter::new(def, config.clone())));
    }

    // Auto-discover tools from registries (lower precedence than config)
    let reg_mgr = RegistryManager::from_config(&config);
    for ext in reg_mgr.list_tools() {
        if configured_tool_names.iter().any(|n| n == &ext.name) {
            continue;
        }
        if !ext.script_path.exists() {
            continue;
        }
        let tool_cfg = crate::config::ScriptToolConfig {
            path: ext.script_path.clone(),
            timeout: 30,
            extra: toml::Table::new(),
        };
        match crate::tool_script::load_single_tool(&ext.name, &tool_cfg) {
            Ok(def) => {
                tool_registry.register(Box::new(LuaToolAdapter::new(def, config.clone())));
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to load registry tool '{}': {}",
                    ext.name, e
                );
            }
        }
    }

    // Print registered tools
    let tool_count = tool_registry.len() + extra_tools.len();
    if tool_count > 3 {
        println!("Registered {} tools:", tool_count);
        for t in tool_registry.tools() {
            let tag = if t.is_builtin() { "builtin" } else { "lua" };
            println!("  POST /tools/{} — {} ({})", t.name(), t.description(), tag);
        }
        for t in extra_tools.tools() {
            println!("  POST /tools/{} — {} (rust)", t.name(), t.description());
        }
    }

    // ── Agents ──
    let mut agent_registry = AgentRegistry::from_config(&config)?;

    // Load and register Lua agents from config
    let lua_agents = load_agent_definitions(&config)?;
    let configured_agent_names: Vec<String> = lua_agents.iter().map(|d| d.name.clone()).collect();
    for def in lua_agents {
        agent_registry.register(Box::new(LuaAgentAdapter::new(def, config.clone())));
    }

    // Auto-discover agents from registries (lower precedence than config)
    for ext in reg_mgr.list_agents() {
        if configured_agent_names.iter().any(|n| n == &ext.name) {
            continue;
        }
        if !ext.script_path.exists() {
            continue;
        }
        if ext.script_path.extension().map_or(false, |e| e == "lua") {
            let agent_cfg = crate::config::ScriptAgentConfig {
                path: ext.script_path.clone(),
                timeout: 30,
                extra: toml::Table::new(),
            };
            match crate::agent_script::load_single_agent(&ext.name, &agent_cfg) {
                Ok(def) => {
                    agent_registry.register(Box::new(LuaAgentAdapter::new(def, config.clone())));
                }
                Err(e) => {
                    eprintln!(
                        "Warning: failed to load registry agent '{}': {}",
                        ext.name, e
                    );
                }
            }
        }
    }

    let agent_count = agent_registry.len() + extra_agents.len();
    if agent_count > 0 {
        println!("Registered {} agents:", agent_count);
        for a in agent_registry.agents() {
            println!(
                "  POST /agents/{}/prompt — {} ({})",
                a.name(),
                a.description(),
                a.source()
            );
        }
        for a in extra_agents.agents() {
            println!(
                "  POST /agents/{}/prompt — {} ({})",
                a.name(),
                a.description(),
                a.source()
            );
        }
    }

    let tools = Arc::new(tool_registry);
    let agents = Arc::new(agent_registry);

    let state = AppState {
        config: config.clone(),
        tools: tools.clone(),
        agents: agents.clone(),
    };

    // MCP Streamable HTTP endpoint at /mcp — clone before moving into extra_state
    let mcp_tools = tools.clone();
    let mcp_extra = extra_tools.clone();
    let mcp_agents = agents.clone();
    let mcp_extra_agents = extra_agents.clone();
    let mcp_config = config.clone();

    let extra_state = (extra_tools.clone(), extra_agents);
    let mcp_service = StreamableHttpService::new(
        move || {
            Ok(McpBridge::new(
                mcp_config.clone(),
                mcp_tools.clone(),
                mcp_extra.clone(),
                mcp_agents.clone(),
                mcp_extra_agents.clone(),
            ))
        },
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/tools/list", get(handle_list_tools))
        .route("/tools/{name}", post(handle_tool_call))
        .route("/agents/list", get(handle_list_agents))
        .route("/agents/{name}/prompt", post(handle_resolve_agent))
        .route("/health", get(handle_health))
        .with_state((state, extra_state))
        .nest_service("/mcp", mcp_service)
        .layer(cors);

    println!("MCP server listening on http://{}", bind_addr);
    println!("  MCP endpoint: http://{}/mcp", bind_addr);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ============ Error response ============

/// JSON error response body, matching `docs/SCHEMAS.md` error schema.
#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

/// Inner error detail with a machine-readable code and human-readable message.
#[derive(Serialize)]
struct ErrorDetail {
    /// Machine-readable error code (e.g., `"bad_request"`, `"not_found"`).
    code: String,
    /// Human-readable error message.
    message: String,
}

/// Internal error type that converts into an Axum HTTP response.
struct AppError {
    status: StatusCode,
    code: String,
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code,
                message: self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

/// Constructs a 400 Bad Request error.
fn bad_request(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::BAD_REQUEST,
        code: "bad_request".to_string(),
        message: message.into(),
    }
}

/// Constructs a 404 Not Found error.
fn not_found(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::NOT_FOUND,
        code: "not_found".to_string(),
        message: message.into(),
    }
}

/// Constructs a 408 Request Timeout error.
fn timeout_error(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::REQUEST_TIMEOUT,
        code: "timeout".to_string(),
        message: message.into(),
    }
}

/// Constructs a 500 error for tool execution failures.
fn tool_error(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "tool_error".to_string(),
        message: message.into(),
    }
}

/// Inspects tool execution errors and maps them to the most appropriate
/// HTTP status code. This allows built-in tools to signal client errors
/// (e.g. empty query → 400, document not found → 404) without needing
/// a custom error type in the `Tool` trait.
fn classify_tool_error(tool_name: &str, err: anyhow::Error) -> AppError {
    let msg = err.to_string();

    if msg.contains("not found") {
        not_found(format!("{}: {}", tool_name, msg))
    } else if msg.contains("must not be empty")
        || msg.contains("embeddings")
        || msg.contains("disabled")
        || msg.contains("invalid")
    {
        // Validation / configuration errors → 400
        let mut e = bad_request(format!("{}: {}", tool_name, msg));
        // Preserve more specific error codes for known patterns
        if msg.contains("embeddings") || msg.contains("disabled") {
            e.code = "embeddings_disabled".to_string();
        }
        e
    } else if msg.contains("timed out") {
        timeout_error(format!("{}: {}", tool_name, msg))
    } else {
        tool_error(format!("{}: {}", tool_name, msg))
    }
}

// ============ GET /health ============

/// JSON response body for `GET /health`.
#[derive(Serialize)]
struct HealthResponse {
    /// Always `"ok"` when the server is running.
    status: String,
    /// The crate version from `Cargo.toml`.
    version: String,
}

/// Handler for `GET /health`.
///
/// Returns a simple health check response with the server status and version.
/// This endpoint is used by load balancers and monitoring tools.
async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ============ GET /tools/list ============

/// JSON response body for `GET /tools/list`.
#[derive(Serialize)]
struct ToolListResponse {
    /// All registered tools.
    tools: Vec<ToolInfo>,
}

/// Handler for `GET /tools/list`.
///
/// Returns all registered tools with their OpenAI function-calling parameter
/// schemas. Built-in tools have `builtin: true`; Lua and custom Rust tools
/// have `builtin: false`.
async fn handle_list_tools(
    State((state, (extra_tools, _extra_agents))): State<(AppState, ExtState)>,
) -> Json<ToolListResponse> {
    let mut tools: Vec<ToolInfo> = state
        .tools
        .tools()
        .iter()
        .map(|t| ToolInfo {
            name: t.name().to_string(),
            description: t.description().to_string(),
            builtin: t.is_builtin(),
            parameters: t.parameters_schema(),
        })
        .collect();

    // Append extra custom Rust tools
    for t in extra_tools.tools() {
        tools.push(ToolInfo {
            name: t.name().to_string(),
            description: t.description().to_string(),
            builtin: false,
            parameters: t.parameters_schema(),
        });
    }

    Json(ToolListResponse { tools })
}

// ============ POST /tools/{name} ============

/// Handler for `POST /tools/{name}`.
///
/// Unified tool dispatch. Looks up the tool by name in the registry
/// (checking the main registry first, then extras), validates parameters,
/// and executes it.
///
/// Returns `404` if the tool is not found, `400` for parameter validation
/// errors, `408` for timeout, and `500` for execution errors.
async fn handle_tool_call(
    State((state, (extra_tools, _extra_agents))): State<(AppState, ExtState)>,
    Path(name): Path<String>,
    Json(params): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Look up the tool in the main registry, then extras
    let tool = state
        .tools
        .find(&name)
        .or_else(|| extra_tools.find(&name))
        .ok_or_else(|| not_found(format!("no tool registered with name: {}", name)))?;

    // Validate parameters against the tool's schema
    let validated_params = validate_params(&tool.parameters_schema(), &params)
        .map_err(|e| bad_request(e.to_string()))?;

    // Execute via the Tool trait
    let ctx = ToolContext::new(state.config.clone());
    let result = tool
        .execute(validated_params, &ctx)
        .await
        .map_err(|e| classify_tool_error(&name, e))?;

    Ok(Json(serde_json::json!({ "result": result })))
}

// ============ GET /agents/list ============

/// JSON response body for `GET /agents/list`.
#[derive(Serialize)]
struct AgentListResponse {
    /// All registered agents.
    agents: Vec<AgentInfo>,
}

/// Handler for `GET /agents/list`.
///
/// Returns all registered agents with their metadata, tool lists, and
/// argument schemas. Includes TOML, Lua, and custom Rust agents.
async fn handle_list_agents(
    State((state, (_extra_tools, extra_agents))): State<(AppState, ExtState)>,
) -> Json<AgentListResponse> {
    let mut agents: Vec<AgentInfo> = state
        .agents
        .agents()
        .iter()
        .map(|a| AgentInfo {
            name: a.name().to_string(),
            description: a.description().to_string(),
            tools: a.tools(),
            source: a.source().to_string(),
            arguments: a.arguments(),
        })
        .collect();

    // Append extra custom Rust agents
    for a in extra_agents.agents() {
        agents.push(AgentInfo {
            name: a.name().to_string(),
            description: a.description().to_string(),
            tools: a.tools(),
            source: a.source().to_string(),
            arguments: a.arguments(),
        });
    }

    Json(AgentListResponse { agents })
}

// ============ POST /agents/{name}/prompt ============

/// Handler for `POST /agents/{name}/prompt`.
///
/// Resolves an agent's system prompt by calling its `resolve()` method.
/// For TOML agents, this returns the static prompt. For Lua agents, this
/// executes the script's `agent.resolve()` function with the provided
/// arguments and access to the context bridge (search, get, sources).
///
/// Returns `404` if the agent is not found.
async fn handle_resolve_agent(
    State((state, (_extra_tools, extra_agents))): State<(AppState, ExtState)>,
    Path(name): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let agent = state
        .agents
        .find(&name)
        .or_else(|| extra_agents.find(&name))
        .ok_or_else(|| not_found(format!("no agent registered with name: {}", name)))?;

    let ctx = ToolContext::new(state.config.clone());
    let prompt = agent
        .resolve(args, &ctx)
        .await
        .map_err(|e| tool_error(format!("agent '{}': {}", name, e)))?;

    Ok(Json(serde_json::to_value(prompt).map_err(|e| {
        tool_error(format!("failed to serialize agent prompt: {}", e))
    })?))
}
