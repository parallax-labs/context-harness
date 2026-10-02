//! MCP-compatible HTTP server.
//!
//! Exposes Context Harness functionality via a JSON HTTP API suitable for
//! integration with Cursor, Claude, and other MCP-compatible AI tools.
//!
//! All tools — built-in (search, get, sources), Lua scripts, and custom Rust
//! trait implementations — are registered in a unified [`ToolRegistry`] and
//! dispatched through the same `POST /tools/{name}` handler.
//!
//! Profiles (named personas with system prompts and tool scoping) are registered
//! in a [`ProfileRegistry`] and discoverable/resolvable via dedicated endpoints.
//!
//! # Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `GET`  | `/tools/list` | List all registered tools with schemas |
//! | `POST` | `/tools/{name}` | Call any registered tool by name |
//! | `GET`  | `/profiles/list` | List all registered profiles with metadata |
//! | `POST` | `/profiles/{name}/prompt` | Resolve a profile's system prompt |
//! | `GET`  | `/agents/list` | Deprecated compatibility alias |
//! | `POST` | `/agents/{name}/prompt` | Deprecated compatibility alias |
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

use crate::config::Config;
use crate::mcp::McpBridge;
use crate::profile_script::{load_profile_definitions, LuaProfileAdapter};
use crate::profiles::{ProfileInfo, ProfileRegistry};
use crate::registry::RegistryManager;
use crate::tool_script::{load_tool_definitions, validate_params, LuaToolAdapter, ToolInfo};
use crate::traits::{ToolContext, ToolRegistry};
use crate::workspace::{RouterError, ServerMode, WorkspaceRouter};

/// Shared application state passed to all route handlers via Axum's `State` extractor.
#[derive(Clone)]
struct AppState {
    /// Workspace router (one-workspace in compatibility mode; multi under
    /// `--workspaces`). Handlers build a [`ToolContext`] from this per request.
    router: Arc<WorkspaceRouter>,
    /// Whether the server emits flat (compat) or workspace-labeled (multi) shapes.
    mode: ServerMode,
    /// Unified tool registry containing built-in, Lua, and custom Rust tools.
    tools: Arc<ToolRegistry>,
    /// Profile registry containing TOML, Lua, and custom Rust profiles.
    profiles: Arc<ProfileRegistry>,
}

/// Extra extensions (custom Rust tools and profiles) passed alongside the main `AppState`.
type ExtState = (Arc<ToolRegistry>, Arc<ProfileRegistry>);

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
#[allow(dead_code)] // Public library entry point; the CLI supplies resource directories.
pub async fn run_server(config: &Config) -> anyhow::Result<()> {
    run_server_with_extensions(
        config,
        Arc::new(ToolRegistry::new()),
        Arc::new(ProfileRegistry::new()),
    )
    .await
}

/// Starts the MCP server with custom Rust tool and profile extensions.
///
/// Like [`run_server`], but accepts a [`ToolRegistry`] and [`ProfileRegistry`]
/// containing custom extensions that will be served alongside built-in,
/// TOML-defined, and Lua-scripted entries.
///
/// Custom tools appear in `GET /tools/list` and can be called via
/// `POST /tools/{name}`. Custom profiles appear in `GET /profiles/list` and
/// can be resolved via `POST /profiles/{name}/prompt`.
///
/// # Example
///
/// ```rust,no_run
/// use context_harness::server::run_server_with_extensions;
/// use context_harness::traits::ToolRegistry;
/// use context_harness::profiles::ProfileRegistry;
/// use std::sync::Arc;
///
/// # async fn example(config: &context_harness::config::Config) -> anyhow::Result<()> {
/// let tools = ToolRegistry::new();
/// let profiles = ProfileRegistry::new();
/// run_server_with_extensions(config, Arc::new(tools), Arc::new(profiles)).await?;
/// # Ok(())
/// # }
/// ```
#[allow(dead_code)] // Public library entry point.
pub async fn run_server_with_extensions(
    config: &Config,
    extra_tools: Arc<ToolRegistry>,
    extra_profiles: Arc<ProfileRegistry>,
) -> anyhow::Result<()> {
    run_server_with_resources(config, extra_tools, extra_profiles, &[]).await
}

/// Start a single-workspace server with explicitly selected standalone resources.
/// Resources project static prompts only; no runtime, credentials, or client
/// processes are initialized. Existing library entry points do not discover cwd.
pub async fn run_server_with_resources(
    config: &Config,
    extra_tools: Arc<ToolRegistry>,
    extra_profiles: Arc<ProfileRegistry>,
    directories: &[crate::agent_resource::ResourceDirectory],
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

    // ── Profiles ──
    let mut profile_registry = ProfileRegistry::from_config(&config)?;

    // Load and register Lua profiles from config
    let lua_profiles = load_profile_definitions(&config)?;
    let configured_profile_names: Vec<String> =
        lua_profiles.iter().map(|d| d.name.clone()).collect();
    for def in lua_profiles {
        profile_registry.register(Box::new(LuaProfileAdapter::new(def, config.clone())));
    }

    // Auto-discover legacy prompt extensions from registries (lower precedence than config)
    for ext in reg_mgr.list_profiles() {
        if configured_profile_names.iter().any(|n| n == &ext.name) {
            continue;
        }
        if !ext.script_path.exists() {
            continue;
        }
        if ext.script_path.extension().is_some_and(|e| e == "lua") {
            let profile_cfg = crate::config::ScriptProfileConfig {
                path: ext.script_path.clone(),
                timeout: 30,
                extra: toml::Table::new(),
            };
            match crate::profile_script::load_single_profile(&ext.name, &profile_cfg) {
                Ok(def) => {
                    profile_registry
                        .register(Box::new(LuaProfileAdapter::new(def, config.clone())));
                }
                Err(e) => {
                    eprintln!(
                        "Warning: failed to load registry profile '{}': {}",
                        ext.name, e
                    );
                }
            }
        }
    }

    register_resource_prompts(
        config.as_ref(),
        directories,
        &mut profile_registry,
        &extra_profiles,
    )?;

    let profile_count = profile_registry.len() + extra_profiles.len();
    if profile_count > 0 {
        println!("Registered {} profiles:", profile_count);
        for a in profile_registry.profiles() {
            println!(
                "  POST /profiles/{}/prompt — {} ({})",
                a.name(),
                a.description(),
                a.source()
            );
        }
        for a in extra_profiles.profiles() {
            println!(
                "  POST /profiles/{}/prompt — {} ({})",
                a.name(),
                a.description(),
                a.source()
            );
        }
    }

    let tools = Arc::new(tool_registry);
    let profiles = Arc::new(profile_registry);

    // Compatibility mode is a router with one workspace; the wire contract is
    // selected by mode, not by workspace count (SPEC-0014 R14/R15).
    let router = Arc::new(WorkspaceRouter::single(config.clone()));

    serve_router(
        router,
        ServerMode::Compat,
        bind_addr,
        false,
        tools,
        profiles,
        extra_tools,
        extra_profiles,
    )
    .await
}

/// Starts the multi-workspace MCP server (`ctx serve mcp --workspaces`).
///
/// Routes the built-in `search` / `get` / `sources` / `workspaces` tools across
/// the registered workspaces in `router`. Per SPEC-0014 R54, only built-in
/// tools are exposed in multi-workspace mode in Phase 1 — workspace-local Lua
/// and registry tools/profiles are not loaded. `bind` comes from the registry's
/// `[defaults].bind` (R16); `allow_remote` permits a non-loopback bind.
pub async fn run_server_multi(
    router: Arc<WorkspaceRouter>,
    bind: String,
    allow_remote: bool,
) -> anyhow::Result<()> {
    let tools = Arc::new(ToolRegistry::with_builtins_multi());
    let profiles = Arc::new(ProfileRegistry::new());
    let extra_tools = Arc::new(ToolRegistry::new());
    let extra_profiles = Arc::new(ProfileRegistry::new());

    println!("Multi-workspace MCP mode. Registered workspaces:");
    for rt in router.list() {
        let default = if Some(rt.id.as_str()) == router.default_id() {
            " (default)"
        } else {
            ""
        };
        let health = if rt.health.is_ok() {
            "ok"
        } else {
            "unavailable"
        };
        println!(
            "  {}{} — enabled={} health={} resolution={}",
            rt.id,
            default,
            rt.enabled,
            health,
            rt.resolution.as_str()
        );
    }

    serve_router(
        router,
        ServerMode::Multi,
        bind,
        allow_remote,
        tools,
        profiles,
        extra_tools,
        extra_profiles,
    )
    .await
}

/// Whether a `host:port` bind address targets the loopback interface.
fn is_loopback_bind(bind: &str) -> bool {
    let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or(bind);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Shared Axum wiring for both compatibility and multi-workspace modes.
#[allow(clippy::too_many_arguments)]
async fn serve_router(
    router: Arc<WorkspaceRouter>,
    mode: ServerMode,
    bind_addr: String,
    allow_remote: bool,
    tools: Arc<ToolRegistry>,
    profiles: Arc<ProfileRegistry>,
    extra_tools: Arc<ToolRegistry>,
    extra_profiles: Arc<ProfileRegistry>,
) -> anyhow::Result<()> {
    // Trust model (SPEC-0014 trust-model section): loopback bind is the
    // load-bearing control. A non-loopback bind is refused in multi-workspace
    // mode (which fronts every registered store) unless explicitly allowed.
    if !is_loopback_bind(&bind_addr) {
        if mode == ServerMode::Multi && !allow_remote {
            anyhow::bail!(
                "refusing to bind multi-workspace server to non-loopback address '{bind_addr}': \
                 this exposes every registered workspace to other hosts. Bind to 127.0.0.1, or \
                 pass --allow-remote to override."
            );
        }
        eprintln!(
            "warning: binding to non-loopback address '{bind_addr}'. The MCP server has \
             permissive CORS and no authentication; only bind to addresses reachable by \
             trusted clients."
        );
    }

    let state = AppState {
        router: router.clone(),
        mode,
        tools: tools.clone(),
        profiles: profiles.clone(),
    };

    // MCP Streamable HTTP endpoint at /mcp — clone before moving into extra_state
    let mcp_tools = tools.clone();
    let mcp_extra = extra_tools.clone();
    let mcp_profiles = profiles.clone();
    let mcp_extra_profiles = extra_profiles.clone();
    let mcp_router = router.clone();

    let extra_state = (extra_tools.clone(), extra_profiles);
    let mcp_service = StreamableHttpService::new(
        move || {
            Ok(McpBridge::new(
                mcp_router.clone(),
                mode,
                mcp_tools.clone(),
                mcp_extra.clone(),
                mcp_profiles.clone(),
                mcp_extra_profiles.clone(),
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
        .route("/profiles/list", get(handle_list_profiles))
        .route("/profiles/{name}/prompt", post(handle_resolve_profile))
        .route("/agents/list", get(handle_list_agents_compat))
        .route("/agents/{name}/prompt", post(handle_resolve_profile))
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
    // Router errors carry their own SPEC-0014 R64 code; surface it directly.
    if let Some(re) = err.downcast_ref::<RouterError>() {
        let status = match re {
            RouterError::UnknownWorkspace(_) => StatusCode::NOT_FOUND,
            RouterError::WorkspaceUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            RouterError::WorkspaceTimeout { .. } => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_REQUEST,
        };
        return AppError {
            status,
            code: re.code().to_string(),
            message: format!("{tool_name}: {re}"),
        };
    }

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
    State((state, (extra_tools, _extra_profiles))): State<(AppState, ExtState)>,
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
    State((state, (extra_tools, _extra_profiles))): State<(AppState, ExtState)>,
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
    let ctx = ToolContext::routed(state.router.clone(), state.mode);
    let result = tool
        .execute(validated_params, &ctx)
        .await
        .map_err(|e| classify_tool_error(&name, e))?;

    Ok(Json(serde_json::json!({ "result": result })))
}

// ============ Profile endpoints ============

#[derive(Serialize)]
struct ProfileListResponse {
    profiles: Vec<ProfileInfo>,
}

/// Historical response shape retained by `GET /agents/list`.
#[derive(Serialize)]
struct AgentListResponse {
    agents: Vec<ProfileInfo>,
}

fn profile_info(state: &AppState, extra_profiles: &ProfileRegistry) -> Vec<ProfileInfo> {
    state
        .profiles
        .profiles()
        .iter()
        .chain(extra_profiles.profiles().iter())
        .map(|profile| ProfileInfo {
            name: profile.name().to_string(),
            description: profile.description().to_string(),
            tools: profile.tools(),
            source: profile.source().to_string(),
            arguments: profile.arguments(),
        })
        .collect()
}

/// Canonical profile discovery endpoint.
async fn handle_list_profiles(
    State((state, (_extra_tools, extra_profiles))): State<(AppState, ExtState)>,
) -> Json<ProfileListResponse> {
    Json(ProfileListResponse {
        profiles: profile_info(&state, &extra_profiles),
    })
}

/// Deprecated prompt-agent discovery endpoint with its historical JSON key.
async fn handle_list_agents_compat(
    State((state, (_extra_tools, extra_profiles))): State<(AppState, ExtState)>,
) -> Json<AgentListResponse> {
    Json(AgentListResponse {
        agents: profile_info(&state, &extra_profiles),
    })
}

/// Resolve a profile via the canonical route or its legacy agent alias.
async fn handle_resolve_profile(
    State((state, (_extra_tools, extra_profiles))): State<(AppState, ExtState)>,
    Path(name): Path<String>,
    Json(args): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let profile = state
        .profiles
        .find(&name)
        .or_else(|| extra_profiles.find(&name))
        .ok_or_else(|| not_found(format!("no profile registered with name: {}", name)))?;

    let ctx = ToolContext::routed(state.router.clone(), state.mode);
    let prompt = profile
        .resolve(args, &ctx)
        .await
        .map_err(|e| tool_error(format!("profile '{}': {}", name, e)))?;

    Ok(Json(serde_json::to_value(prompt).map_err(|e| {
        tool_error(format!("failed to serialize profile prompt: {}", e))
    })?))
}

/// Register standalone prompt projections, rejecting collisions with all legacy
/// sources (including registry Lua and caller-provided Rust profiles) atomically.
pub fn register_resource_prompts(
    config: &Config,
    directories: &[crate::agent_resource::ResourceDirectory],
    profiles: &mut ProfileRegistry,
    extra_profiles: &ProfileRegistry,
) -> anyhow::Result<()> {
    let resources = crate::agent_resource::load_resources(directories, config)?;
    for name in resources.keys() {
        anyhow::ensure!(
            profiles.find(name).is_none() && extra_profiles.find(name).is_none(),
            "agent resource conflicts with registered profile '{name}'; rename the resource or profile"
        );
    }
    for resource in resources.into_values() {
        profiles.register(Box::new(resource.definition.prompt_profile()));
    }
    Ok(())
}
