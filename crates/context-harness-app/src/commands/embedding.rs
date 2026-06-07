use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tauri::{Emitter, State};

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingStatus {
    pub provider: String,
    pub model: Option<String>,
    pub total_chunks: u64,
    pub embedded_chunks: u64,
    pub pending_chunks: u64,
    pub stale_chunks: u64,
    pub model_status: String,
    pub model_error: Option<String>,
}

#[tauri::command]
pub async fn embedding_status(state: State<'_, AppState>) -> Result<EmbeddingStatus, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks")
        .fetch_one(&workspace.pool)
        .await
        .unwrap_or(0);

    let embedded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_vectors")
        .fetch_one(&workspace.pool)
        .await
        .unwrap_or(0);

    let stale: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM embeddings e \
         JOIN chunks c ON c.id = e.chunk_id \
         WHERE e.hash != c.hash",
    )
    .fetch_one(&workspace.pool)
    .await
    .unwrap_or(0);

    let pending = total - embedded;

    let (model_status, model_error) =
        match context_harness::embedding::create_provider(&workspace.config.embedding) {
            Ok(p) => (format!("ready ({})", p.model_name()), None),
            Err(e) => {
                let msg = e.to_string();
                if workspace.config.embedding.provider == "disabled" {
                    ("disabled".to_string(), None)
                } else {
                    ("error".to_string(), Some(msg))
                }
            }
        };

    Ok(EmbeddingStatus {
        provider: workspace.config.embedding.provider.clone(),
        model: workspace.config.embedding.model.clone(),
        total_chunks: total as u64,
        embedded_chunks: embedded as u64,
        pending_chunks: pending.max(0) as u64,
        stale_chunks: stale as u64,
        model_status,
        model_error,
    })
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

struct PendingChunk {
    chunk_id: String,
    document_id: String,
    text: String,
    text_hash: String,
}

async fn find_pending_chunks(
    pool: &sqlx::SqlitePool,
    model: &str,
) -> Result<Vec<PendingChunk>, AppError> {
    let rows = sqlx::query(
        "SELECT c.id AS chunk_id, c.document_id, c.text \
         FROM chunks c \
         LEFT JOIN embeddings e ON e.chunk_id = c.id AND e.model = ? \
         WHERE e.chunk_id IS NULL OR e.hash != c.hash \
         ORDER BY c.document_id, c.chunk_index",
    )
    .bind(model)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(rows
        .iter()
        .map(|row| {
            let text: String = row.get("text");
            let text_hash = hash_text(&text);
            PendingChunk {
                chunk_id: row.get("chunk_id"),
                document_id: row.get("document_id"),
                text,
                text_hash,
            }
        })
        .collect())
}

async fn upsert_embedding(
    pool: &sqlx::SqlitePool,
    chunk_id: &str,
    document_id: &str,
    model: &str,
    dims: usize,
    text_hash: &str,
    blob: &[u8],
) -> Result<(), AppError> {
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO embeddings (chunk_id, model, dims, created_at, hash) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(chunk_id) DO UPDATE SET \
             model = excluded.model, \
             dims = excluded.dims, \
             created_at = excluded.created_at, \
             hash = excluded.hash",
    )
    .bind(chunk_id)
    .bind(model)
    .bind(dims as i64)
    .bind(now)
    .bind(text_hash)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query(
        "INSERT INTO chunk_vectors (chunk_id, document_id, embedding) \
         VALUES (?, ?, ?) \
         ON CONFLICT(chunk_id) DO UPDATE SET \
             document_id = excluded.document_id, \
             embedding = excluded.embedding",
    )
    .bind(chunk_id)
    .bind(document_id)
    .bind(blob)
    .execute(pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(())
}

#[tauri::command]
pub async fn embedding_run_pending(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<String, AppError> {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let op_id = operation_id.clone();

    let config = {
        let ws = state.workspace.read().await;
        let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;
        workspace.config.clone()
    };

    tokio::spawn(async move {
        let start = std::time::Instant::now();

        let _ = app.emit(
            "embed-progress",
            serde_json::json!({
                "operation_id": op_id,
                "phase": "initializing",
                "message": format!("Initializing {} embedding provider...", config.embedding.provider),
            }),
        );

        let provider = match context_harness::embedding::create_provider(&config.embedding) {
            Ok(p) => p,
            Err(e) => {
                let _ = app.emit(
                    "embed-progress",
                    serde_json::json!({
                        "operation_id": op_id,
                        "phase": "error",
                        "message": format!("Provider error: {e}"),
                    }),
                );
                return;
            }
        };

        let model_name = provider.model_name().to_string();
        let dims = provider.dims();

        let pool = match context_harness::db::connect(&config).await {
            Ok(p) => p,
            Err(e) => {
                let _ = app.emit(
                    "embed-progress",
                    serde_json::json!({
                        "operation_id": op_id,
                        "phase": "error",
                        "message": format!("Database error: {e}"),
                    }),
                );
                return;
            }
        };

        let pending = match find_pending_chunks(&pool, &model_name).await {
            Ok(p) => p,
            Err(e) => {
                let _ = app.emit(
                    "embed-progress",
                    serde_json::json!({
                        "operation_id": op_id,
                        "phase": "error",
                        "message": format!("Failed to find pending chunks: {e}"),
                    }),
                );
                return;
            }
        };

        if pending.is_empty() {
            let _ = app.emit(
                "embed-progress",
                serde_json::json!({
                    "operation_id": op_id,
                    "phase": "complete",
                    "message": "All chunks already embedded",
                }),
            );
            return;
        }

        let total = pending.len();
        let batch_size = config.embedding.batch_size;
        let mut embedded = 0u64;
        let mut failed = 0u64;
        let mut last_error = String::new();

        let _ = app.emit(
            "embed-progress",
            serde_json::json!({
                "operation_id": op_id,
                "phase": "embedding",
                "message": format!("Embedding {total} chunks (batch size {batch_size})..."),
            }),
        );

        for (batch_idx, batch) in pending.chunks(batch_size).enumerate() {
            let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();

            match context_harness::embedding::embed_texts(provider.as_ref(), &config.embedding, &texts).await {
                Ok(vectors) => {
                    for (item, vec) in batch.iter().zip(vectors.iter()) {
                        let blob = context_harness::embedding::vec_to_blob(vec);
                        match upsert_embedding(
                            &pool,
                            &item.chunk_id,
                            &item.document_id,
                            &model_name,
                            dims,
                            &item.text_hash,
                            &blob,
                        )
                        .await
                        {
                            Ok(()) => embedded += 1,
                            Err(e) => {
                                failed += 1;
                                last_error = e.to_string();
                            }
                        }
                    }

                    let done = (batch_idx + 1) * batch_size;
                    let progress = done.min(total);
                    let _ = app.emit(
                        "embed-progress",
                        serde_json::json!({
                            "operation_id": op_id,
                            "phase": "embedding",
                            "message": format!("Embedded {progress}/{total} chunks ({embedded} ok, {failed} failed)"),
                        }),
                    );
                }
                Err(e) => {
                    failed += batch.len() as u64;
                    last_error = e.to_string();
                    let _ = app.emit(
                        "embed-progress",
                        serde_json::json!({
                            "operation_id": op_id,
                            "phase": "embedding",
                            "message": format!("Batch failed: {e}"),
                        }),
                    );
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        pool.close().await;

        if embedded > 0 {
            let _ = app.emit(
                "embed-progress",
                serde_json::json!({
                    "operation_id": op_id,
                    "phase": "complete",
                    "elapsed_ms": (elapsed * 1000.0) as u64,
                    "message": format!("Embedded {embedded}/{total} chunks in {elapsed:.1}s{}", if failed > 0 { format!(" ({failed} failed)") } else { String::new() }),
                }),
            );
        } else {
            let _ = app.emit(
                "embed-progress",
                serde_json::json!({
                    "operation_id": op_id,
                    "phase": "error",
                    "elapsed_ms": (elapsed * 1000.0) as u64,
                    "message": format!("All {total} chunks failed to embed. Last error: {last_error}"),
                }),
            );
        }
    });

    Ok(operation_id)
}

#[tauri::command]
pub async fn embedding_rebuild(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<String, AppError> {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let op_id = operation_id.clone();

    let config = {
        let ws = state.workspace.read().await;
        let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;
        workspace.config.clone()
    };

    tokio::spawn(async move {
        let start = std::time::Instant::now();

        let _ = app.emit(
            "embed-progress",
            serde_json::json!({
                "operation_id": op_id,
                "phase": "initializing",
                "message": format!("Initializing {} provider for rebuild...", config.embedding.provider),
            }),
        );

        let provider = match context_harness::embedding::create_provider(&config.embedding) {
            Ok(p) => p,
            Err(e) => {
                let _ = app.emit(
                    "embed-progress",
                    serde_json::json!({
                        "operation_id": op_id,
                        "phase": "error",
                        "message": format!("Provider error: {e}"),
                    }),
                );
                return;
            }
        };

        let model_name = provider.model_name().to_string();
        let dims = provider.dims();

        let pool = match context_harness::db::connect(&config).await {
            Ok(p) => p,
            Err(e) => {
                let _ = app.emit(
                    "embed-progress",
                    serde_json::json!({
                        "operation_id": op_id,
                        "phase": "error",
                        "message": format!("Database error: {e}"),
                    }),
                );
                return;
            }
        };

        // Clear existing embeddings
        let _ = sqlx::query("DELETE FROM chunk_vectors").execute(&pool).await;
        let _ = sqlx::query("DELETE FROM embeddings").execute(&pool).await;

        let pending = match find_pending_chunks(&pool, &model_name).await {
            Ok(p) => p,
            Err(e) => {
                let _ = app.emit(
                    "embed-progress",
                    serde_json::json!({
                        "operation_id": op_id,
                        "phase": "error",
                        "message": format!("Failed to query chunks: {e}"),
                    }),
                );
                return;
            }
        };

        let total = pending.len();
        let batch_size = config.embedding.batch_size;
        let mut embedded = 0u64;
        let mut failed = 0u64;
        let mut last_error = String::new();

        for (batch_idx, batch) in pending.chunks(batch_size).enumerate() {
            let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();

            match context_harness::embedding::embed_texts(provider.as_ref(), &config.embedding, &texts).await {
                Ok(vectors) => {
                    for (item, vec) in batch.iter().zip(vectors.iter()) {
                        let blob = context_harness::embedding::vec_to_blob(vec);
                        match upsert_embedding(&pool, &item.chunk_id, &item.document_id, &model_name, dims, &item.text_hash, &blob).await {
                            Ok(()) => embedded += 1,
                            Err(e) => {
                                failed += 1;
                                last_error = e.to_string();
                            }
                        }
                    }

                    let done = (batch_idx + 1) * batch_size;
                    let progress = done.min(total);
                    let _ = app.emit(
                        "embed-progress",
                        serde_json::json!({
                            "operation_id": op_id,
                            "phase": "embedding",
                            "message": format!("Rebuilt {progress}/{total} ({embedded} ok, {failed} failed)"),
                        }),
                    );
                }
                Err(e) => {
                    failed += batch.len() as u64;
                    last_error = e.to_string();
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        pool.close().await;

        if embedded > 0 {
            let _ = app.emit(
                "embed-progress",
                serde_json::json!({
                    "operation_id": op_id,
                    "phase": "complete",
                    "message": format!("Rebuilt {embedded}/{total} embeddings in {elapsed:.1}s"),
                }),
            );
        } else if total == 0 {
            let _ = app.emit(
                "embed-progress",
                serde_json::json!({
                    "operation_id": op_id,
                    "phase": "complete",
                    "message": "No chunks to embed",
                }),
            );
        } else {
            let _ = app.emit(
                "embed-progress",
                serde_json::json!({
                    "operation_id": op_id,
                    "phase": "error",
                    "message": format!("All {total} chunks failed to embed. Last error: {last_error}"),
                }),
            );
        }
    });

    Ok(operation_id)
}

#[tauri::command]
pub async fn embedding_update_config(
    state: State<'_, AppState>,
    provider: String,
    model: Option<String>,
    dims: Option<usize>,
) -> Result<(), AppError> {
    let mut ws = state.workspace.write().await;
    let workspace = ws.as_mut().ok_or(AppError::NoWorkspace)?;

    let config_path = workspace.path.join("config").join("ctx.toml");
    let mut contents = std::fs::read_to_string(&config_path)
        .map_err(|e| AppError::Internal(format!("Failed to read config: {e}")))?;

    let embed_section = {
        let mut s = format!("[embedding]\nprovider = \"{provider}\"\n");
        if let Some(m) = &model {
            s.push_str(&format!("model = \"{m}\"\n"));
        }
        if let Some(d) = dims {
            s.push_str(&format!("dims = {d}\n"));
        }
        s
    };

    if let Some(start) = contents.find("[embedding]") {
        let end = contents[start + 1..]
            .find("\n[")
            .map(|i| start + 1 + i)
            .unwrap_or(contents.len());
        contents.replace_range(start..end, &embed_section);
    }

    std::fs::write(&config_path, &contents)
        .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;

    let new_config = super::load_and_resolve_config(&workspace.path, &config_path)
        .map_err(|e| AppError::InvalidConfig(e.to_string()))?;
    workspace.config = new_config;

    Ok(())
}
