//! MCP-compatible HTTP server.
//!
//! Exposes Context Harness functionality via a JSON HTTP API suitable for
//! integration with Cursor, Claude, and other MCP-compatible AI tools.
//!
//! # Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | `POST` | `/tools/search` | Search indexed documents (keyword, semantic, hybrid) |
//! | `POST` | `/tools/get` | Retrieve a document by UUID |
//! | `GET`  | `/tools/sources` | List connector configuration and health |
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
//! `internal` (500).
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
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::config::Config;
use crate::get::{get_document, DocumentResponse};
use crate::search::{search_documents, SearchResultItem};
use crate::sources::{get_sources, SourceStatus};

/// Shared application state passed to all route handlers via Axum's `State` extractor.
#[derive(Clone)]
struct AppState {
    /// Application configuration (wrapped in `Arc` for cheap cloning across handlers).
    config: Arc<Config>,
}

/// Starts the MCP-compatible HTTP server.
///
/// Binds to the address configured in `[server].bind` and registers all
/// route handlers. The server runs indefinitely until the process is terminated.
///
/// # Arguments
///
/// - `config` — application configuration (database path, retrieval settings, bind address).
///
/// # Returns
///
/// Returns `Ok(())` when the server shuts down, or an error if binding fails.
pub async fn run_server(config: &Config) -> anyhow::Result<()> {
    let bind_addr = config.server.bind.clone();

    let state = AppState {
        config: Arc::new(config.clone()),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/tools/search", post(handle_search))
        .route("/tools/get", post(handle_get))
        .route("/tools/sources", get(handle_sources))
        .route("/health", get(handle_health))
        .layer(cors)
        .with_state(state);

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

/// Constructs a 400 error specifically for when semantic/hybrid search
/// is requested but embeddings are disabled.
fn embeddings_disabled(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::BAD_REQUEST,
        code: "embeddings_disabled".to_string(),
        message: message.into(),
    }
}

/// Constructs a 500 Internal Server Error.
fn internal_error(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "internal".to_string(),
        message: message.into(),
    }
}

// ============ POST /tools/search ============

/// JSON request body for `POST /tools/search`.
#[derive(Deserialize)]
struct SearchRequest {
    /// The search query string.
    query: String,
    /// Search mode: `"keyword"`, `"semantic"`, or `"hybrid"`. Defaults to `"keyword"`.
    #[serde(default = "default_mode")]
    mode: String,
    /// Maximum number of results to return. Defaults to 12.
    #[serde(default = "default_search_limit")]
    limit: i64,
    /// Optional filters to narrow results by source, tags, or date range.
    #[serde(default)]
    filters: Option<SearchFilters>,
}

/// Optional filters for search requests.
#[derive(Deserialize, Default)]
struct SearchFilters {
    /// Filter results to a specific connector source (e.g., `"filesystem"`).
    source: Option<String>,
    /// Tag-based filtering (reserved for future use).
    #[allow(dead_code)]
    tags: Option<Vec<String>>,
    /// Only return documents updated on or after this ISO 8601 date.
    since: Option<String>,
    /// Only return documents updated on or before this ISO 8601 date (reserved).
    #[allow(dead_code)]
    until: Option<String>,
}

fn default_mode() -> String {
    "keyword".to_string()
}

fn default_search_limit() -> i64 {
    12
}

/// JSON response body for `POST /tools/search`.
#[derive(Serialize)]
struct SearchResponse {
    /// Ranked list of search results.
    results: Vec<SearchResultItem>,
}

/// Handler for `POST /tools/search`.
///
/// Validates the request, dispatches to [`search_documents`], and returns
/// ranked results. Returns `400` for empty queries, unknown modes, or
/// disabled embeddings; `500` for internal errors.
async fn handle_search(
    State(state): State<AppState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, AppError> {
    if req.query.trim().is_empty() {
        return Err(bad_request("query must not be empty"));
    }

    match req.mode.as_str() {
        "keyword" | "semantic" | "hybrid" => {}
        _ => {
            return Err(bad_request(format!(
                "Unknown search mode: {}. Use keyword, semantic, or hybrid.",
                req.mode
            )))
        }
    }

    let filters = req.filters.unwrap_or_default();

    let results = search_documents(
        &state.config,
        &req.query,
        &req.mode,
        filters.source.as_deref(),
        filters.since.as_deref(),
        Some(req.limit),
    )
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("embeddings") {
            embeddings_disabled(msg)
        } else {
            internal_error(msg)
        }
    })?;

    Ok(Json(SearchResponse { results }))
}

// ============ POST /tools/get ============

/// JSON request body for `POST /tools/get`.
#[derive(Deserialize)]
struct GetRequest {
    /// The UUID of the document to retrieve.
    id: String,
}

/// Handler for `POST /tools/get`.
///
/// Retrieves a full document by UUID, including metadata, body, and all chunks.
/// Returns `404` if the document is not found; `500` for internal errors.
async fn handle_get(
    State(state): State<AppState>,
    Json(req): Json<GetRequest>,
) -> Result<Json<DocumentResponse>, AppError> {
    if req.id.trim().is_empty() {
        return Err(bad_request("id must not be empty"));
    }

    let doc = get_document(&state.config, &req.id).await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            not_found(msg)
        } else {
            internal_error(msg)
        }
    })?;

    Ok(Json(doc))
}

// ============ GET /tools/sources ============

/// JSON response body for `GET /tools/sources`.
#[derive(Serialize)]
struct SourcesResponse {
    /// List of all known connectors and their status.
    sources: Vec<SourceStatus>,
}

/// Handler for `GET /tools/sources`.
///
/// Returns the configuration and health status of all connectors.
async fn handle_sources(State(state): State<AppState>) -> Result<Json<SourcesResponse>, AppError> {
    let sources = get_sources(&state.config);
    Ok(Json(SourcesResponse { sources }))
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

// ============ Response types need Serialize on imported types ============
// SearchResultItem, DocumentResponse, ChunkResponse, SourceStatus already derive Serialize
