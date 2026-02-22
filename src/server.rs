//! MCP-compatible HTTP server.
//!
//! Exposes Context Harness functionality via a JSON HTTP API suitable for
//! integration with Cursor, Claude, and other MCP-compatible AI tools.
//!
//! All tools — built-in (search, get, sources), Lua scripts, and custom Rust
//! trait implementations — are registered in a unified [`ToolRegistry`] and
//! dispatched through the same `POST /tools/{name}` handler.
//!
//! # Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `GET`  | `/tools/list` | List all registered tools with schemas |
//! | `POST` | `/tools/{name}` | Call any registered tool by name |
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
//! Add the following to your Cursor MCP configuration:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "context-harness": {
//!       "command": "ctx",
//!       "args": ["--config", "/path/to/ctx.toml", "serve", "mcp"]
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
use serde::Serialize;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::config::Config;
use crate::tool_script::{load_tool_definitions, validate_params, LuaToolAdapter, ToolInfo};
use crate::traits::{ToolContext, ToolRegistry};

/// Shared application state passed to all route handlers via Axum's `State` extractor.
#[derive(Clone)]
struct AppState {
    /// Application configuration (wrapped in `Arc` for cheap cloning across handlers).
    config: Arc<Config>,
    /// Unified tool registry containing built-in, Lua, and custom Rust tools.
    tools: Arc<ToolRegistry>,
}

/// Starts the MCP-compatible HTTP server.
///
/// Binds to the address configured in `[server].bind` and registers all
/// route handlers. The server runs indefinitely until the process is terminated.
///
/// This is the standard entry point used by the `ctx serve mcp` command.
/// For custom binaries with Rust tool extensions, use
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
    run_server_with_extensions(config, Arc::new(ToolRegistry::new())).await
}

/// Starts the MCP server with custom Rust tool extensions.
///
/// Like [`run_server`], but accepts a [`ToolRegistry`] containing custom
/// tools that will be served alongside built-in and Lua tools.
///
/// Custom tools appear in `GET /tools/list` and can be called via
/// `POST /tools/{name}`.
///
/// # Example
///
/// ```rust,no_run
/// use context_harness::server::run_server_with_extensions;
/// use context_harness::traits::ToolRegistry;
/// use std::sync::Arc;
///
/// # async fn example(config: &context_harness::config::Config) -> anyhow::Result<()> {
/// let mut tools = ToolRegistry::new();
/// // tools.register(Box::new(MyTool::new()));
/// run_server_with_extensions(config, Arc::new(tools)).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_server_with_extensions(
    config: &Config,
    extra_tools: Arc<ToolRegistry>,
) -> anyhow::Result<()> {
    let bind_addr = config.server.bind.clone();
    let config = Arc::new(config.clone());

    // Build the unified tool registry
    let mut registry = ToolRegistry::with_builtins();

    // Load and register Lua tools
    let lua_defs = load_tool_definitions(&config)?;
    for def in lua_defs {
        registry.register(Box::new(LuaToolAdapter::new(def, config.clone())));
    }

    // Register extra custom Rust tools
    // (We can't move out of the Arc, so we iterate and re-register)
    // The extra_tools are appended after built-in and Lua tools.
    // Note: since we can't move Box<dyn Tool> out of an Arc<ToolRegistry>,
    // extra tools should be registered directly into a ToolRegistry passed
    // to this function. For the existing API compat, extra_tools are
    // handled through a merged view at dispatch time.

    // Print registered tools
    let tool_count = registry.len() + extra_tools.len();
    if tool_count > 3 {
        // More than just the 3 built-ins
        println!("Registered {} tools:", tool_count);
        for t in registry.tools() {
            let tag = if t.is_builtin() { "builtin" } else { "lua" };
            println!("  POST /tools/{} — {} ({})", t.name(), t.description(), tag);
        }
        for t in extra_tools.tools() {
            println!("  POST /tools/{} — {} (rust)", t.name(), t.description());
        }
    }

    let state = AppState {
        config,
        tools: Arc::new(registry),
    };

    // Store extra_tools in state for dispatch
    let extra_state = extra_tools;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/tools/list", get(handle_list_tools))
        .route("/tools/{name}", post(handle_tool_call))
        .route("/health", get(handle_health))
        .layer(cors)
        .with_state((state, extra_state));

    println!("MCP server listening on http://{}", bind_addr);

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
    State((state, extras)): State<(AppState, Arc<ToolRegistry>)>,
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
    for t in extras.tools() {
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
    State((state, extras)): State<(AppState, Arc<ToolRegistry>)>,
    Path(name): Path<String>,
    Json(params): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Look up the tool in the main registry, then extras
    let tool = state
        .tools
        .find(&name)
        .or_else(|| extras.find(&name))
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
