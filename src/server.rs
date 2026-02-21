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

/// Shared application state.
#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
}

/// Start the MCP-compatible HTTP server.
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

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

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

fn bad_request(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::BAD_REQUEST,
        code: "bad_request".to_string(),
        message: message.into(),
    }
}

fn not_found(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::NOT_FOUND,
        code: "not_found".to_string(),
        message: message.into(),
    }
}

fn embeddings_disabled(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::BAD_REQUEST,
        code: "embeddings_disabled".to_string(),
        message: message.into(),
    }
}

fn internal_error(message: impl Into<String>) -> AppError {
    AppError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "internal".to_string(),
        message: message.into(),
    }
}

// ============ POST /tools/search ============

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_search_limit")]
    limit: i64,
    #[serde(default)]
    filters: Option<SearchFilters>,
}

#[derive(Deserialize, Default)]
struct SearchFilters {
    source: Option<String>,
    #[allow(dead_code)]
    tags: Option<Vec<String>>,
    since: Option<String>,
    #[allow(dead_code)]
    until: Option<String>,
}

fn default_mode() -> String {
    "keyword".to_string()
}

fn default_search_limit() -> i64 {
    12
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResultItem>,
}

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

#[derive(Deserialize)]
struct GetRequest {
    id: String,
}

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

#[derive(Serialize)]
struct SourcesResponse {
    sources: Vec<SourceStatus>,
}

async fn handle_sources(State(state): State<AppState>) -> Result<Json<SourcesResponse>, AppError> {
    let sources = get_sources(&state.config);
    Ok(Json(SourcesResponse { sources }))
}

// ============ GET /health ============

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ============ Response types need Serialize on imported types ============
// SearchResultItem, DocumentResponse, ChunkResponse, SourceStatus already derive Serialize
