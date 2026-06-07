use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tauri::{Emitter, State};
use uuid::Uuid;

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct DocumentItem {
    pub id: String,
    pub title: Option<String>,
    pub source: String,
    pub source_id: String,
    pub source_url: Option<String>,
    pub updated_at: String,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentListResponse {
    pub documents: Vec<DocumentItem>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChunkInfo {
    pub id: String,
    pub index: i64,
    pub text: String,
    pub content_hash: String,
    pub has_embedding: bool,
}

#[tauri::command]
pub async fn document_get(
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let doc = context_harness::get::get_document(&workspace.config, &id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    serde_json::to_value(&doc).map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn document_list(
    state: State<'_, AppState>,
    source: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<DocumentListResponse, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let lim = limit.unwrap_or(50);
    let off = offset.unwrap_or(0);

    let (total, rows) = if let Some(src) = &source {
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE source = ?")
                .bind(src)
                .fetch_one(&workspace.pool)
                .await
                .unwrap_or(0);

        let rows = sqlx::query(
            "SELECT id, title, source, source_id, source_url, updated_at, content_type \
             FROM documents WHERE source = ? ORDER BY updated_at DESC LIMIT ? OFFSET ?",
        )
        .bind(src)
        .bind(lim)
        .bind(off)
        .fetch_all(&workspace.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        (total, rows)
    } else {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
            .fetch_one(&workspace.pool)
            .await
            .unwrap_or(0);

        let rows = sqlx::query(
            "SELECT id, title, source, source_id, source_url, updated_at, content_type \
             FROM documents ORDER BY updated_at DESC LIMIT ? OFFSET ?",
        )
        .bind(lim)
        .bind(off)
        .fetch_all(&workspace.pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        (total, rows)
    };

    let documents = rows
        .iter()
        .map(|row| {
            let ts: i64 = row.get("updated_at");
            let updated_at = chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_else(|| ts.to_string());

            DocumentItem {
                id: row.get("id"),
                title: row.get("title"),
                source: row.get("source"),
                source_id: row.get("source_id"),
                source_url: row.get("source_url"),
                updated_at,
                content_type: row.get("content_type"),
            }
        })
        .collect();

    Ok(DocumentListResponse {
        documents,
        total: total as u64,
    })
}

#[tauri::command]
pub async fn document_chunks(
    state: State<'_, AppState>,
    document_id: String,
) -> Result<Vec<ChunkInfo>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let rows = sqlx::query(
        "SELECT c.id, c.chunk_index, c.text, c.hash, \
         (cv.chunk_id IS NOT NULL) AS has_embedding \
         FROM chunks c \
         LEFT JOIN chunk_vectors cv ON cv.chunk_id = c.id \
         WHERE c.document_id = ? \
         ORDER BY c.chunk_index",
    )
    .bind(&document_id)
    .fetch_all(&workspace.pool)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    let chunks = rows
        .iter()
        .map(|row| ChunkInfo {
            id: row.get("id"),
            index: row.get("chunk_index"),
            text: row.get("text"),
            content_hash: row.get("hash"),
            has_embedding: row.get::<bool, _>("has_embedding"),
        })
        .collect();

    Ok(chunks)
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub imported: u64,
    pub failed: u64,
    pub errors: Vec<String>,
    pub connectors_created: Vec<String>,
}

fn content_type_for_ext(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "md" | "markdown" => "text/markdown",
        "txt" | "text" | "log" => "text/plain",
        "rs" | "ts" | "js" | "py" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        | "rb" | "lua" | "sh" | "bash" | "zsh" | "fish" | "css" | "scss"
        | "html" | "xml" | "json" | "yaml" | "yml" | "toml" | "ini" | "cfg"
        | "sql" | "graphql" | "proto" | "swift" | "kt" | "scala" | "r"
        | "svelte" | "vue" | "jsx" | "tsx" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[tauri::command]
pub async fn document_import(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<ImportResult, AppError> {
    let (max_tokens, pool, has_root_connector, workspace_path, config_path) = {
        let ws = state.workspace.read().await;
        let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;
        let ws_root_str = workspace.path.to_string_lossy().to_string();
        let has_root = workspace
            .config
            .connectors
            .filesystem
            .values()
            .any(|fc| fc.root.to_string_lossy().to_string() == ws_root_str);
        (
            workspace.config.chunking.max_tokens,
            workspace.pool.clone(),
            has_root,
            workspace.path.clone(),
            workspace.path.join("config").join("ctx.toml"),
        )
    };

    let documents_dir = workspace_path.join("documents");
    std::fs::create_dir_all(&documents_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create documents dir: {e}")))?;

    let total = paths.len() as u64;
    let mut imported = 0u64;
    let mut failed = 0u64;
    let mut errors = Vec::new();
    let mut copied_any = false;

    for (i, path_str) in paths.iter().enumerate() {
        let src_path = PathBuf::from(path_str);

        // If the file is outside the workspace, copy it in
        let path = if !src_path.starts_with(&workspace_path) {
            let file_name_os = src_path
                .file_name()
                .ok_or_else(|| AppError::Internal("No filename".to_string()))?;
            let dest = documents_dir.join(file_name_os);
            if let Err(e) = std::fs::copy(&src_path, &dest) {
                failed += 1;
                errors.push(format!("{}: copy failed: {e}", file_name_os.to_string_lossy()));
                continue;
            }
            copied_any = true;
            dest
        } else {
            src_path.clone()
        };

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.clone());

        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let ct = content_type_for_ext(&ext);

        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                failed += 1;
                errors.push(format!("{file_name}: {e}"));
                continue;
            }
        };

        let body = if ct.starts_with("text/") {
            String::from_utf8_lossy(&bytes).to_string()
        } else if ct == "application/octet-stream" {
            failed += 1;
            errors.push(format!("{file_name}: unsupported file type (.{ext})"));
            continue;
        } else {
            match context_harness::extract::extract_text(&bytes, ct) {
                Ok(text) => text,
                Err(e) => {
                    failed += 1;
                    errors.push(format!("{file_name}: extraction failed: {e}"));
                    continue;
                }
            }
        };

        if body.trim().is_empty() {
            failed += 1;
            errors.push(format!("{file_name}: no text content extracted"));
            continue;
        }

        let now = chrono::Utc::now();
        let source = "import".to_string();
        let source_id = file_name.clone();

        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hasher.update(source_id.as_bytes());
        hasher.update(now.timestamp().to_le_bytes());
        hasher.update(body.as_bytes());
        let dedup_hash = format!("{:x}", hasher.finalize());

        let existing_id: Option<String> =
            sqlx::query_scalar("SELECT id FROM documents WHERE source = ? AND source_id = ?")
                .bind(&source)
                .bind(&source_id)
                .fetch_optional(&pool)
                .await
                .unwrap_or(None);

        let doc_id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let upsert_result = sqlx::query(
            r#"INSERT INTO documents (id, source, source_id, source_url, title, author, created_at, updated_at, content_type, body, metadata_json, raw_json, dedup_hash)
            VALUES (?, ?, ?, NULL, ?, NULL, ?, ?, ?, ?, '{}', NULL, ?)
            ON CONFLICT(source, source_id) DO UPDATE SET
                title = excluded.title,
                updated_at = excluded.updated_at,
                content_type = excluded.content_type,
                body = excluded.body,
                dedup_hash = excluded.dedup_hash"#,
        )
        .bind(&doc_id)
        .bind(&source)
        .bind(&source_id)
        .bind(&file_name)
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(ct)
        .bind(&body)
        .bind(&dedup_hash)
        .execute(&pool)
        .await;

        if let Err(e) = upsert_result {
            failed += 1;
            errors.push(format!("{file_name}: db error: {e}"));
            continue;
        }

        let chunks = context_harness::chunk::chunk_text(&doc_id, &body, max_tokens);

        let chunk_result = replace_doc_chunks(&pool, &doc_id, &chunks).await;
        if let Err(e) = chunk_result {
            failed += 1;
            errors.push(format!("{file_name}: chunking error: {e}"));
            continue;
        }

        imported += 1;

        let _ = app.emit(
            "import-progress",
            serde_json::json!({
                "current": i + 1,
                "total": total,
                "file": file_name,
                "chunks": chunks.len(),
            }),
        );
    }

    let mut connectors_created = Vec::new();

    // If files were copied in and the workspace has no root connector,
    // add one so future syncs pick up the workspace contents.
    if !has_root_connector && (copied_any || imported > 0) {
        if let Ok(mut contents) = std::fs::read_to_string(&config_path) {
            let ws_root = workspace_path.to_string_lossy();
            contents.push_str(&format!(
                "\n[connectors.filesystem.workspace]\nroot = \"{ws_root}\"\ninclude_globs = [\"**/*.md\", \"**/*.txt\", \"**/*.pdf\", \"**/*.docx\", \"**/*.pptx\", \"**/*.xlsx\", \"**/*.rs\", \"**/*.ts\", \"**/*.js\", \"**/*.py\", \"**/*.go\", \"**/*.java\", \"**/*.yaml\", \"**/*.yml\", \"**/*.json\", \"**/*.toml\", \"**/*.sh\"]\nexclude_globs = [\"config/**\", \"data/**\"]\n"
            ));
            connectors_created.push("filesystem:workspace".to_string());

            if let Ok(()) = std::fs::write(&config_path, &contents) {
                let mut ws = state.workspace.write().await;
                if let Some(workspace) = ws.as_mut() {
                    if let Ok(new_config) =
                        super::load_and_resolve_config(&workspace_path, &config_path)
                    {
                        workspace.config = new_config;
                    }
                }
            }
        }
    }

    Ok(ImportResult {
        imported,
        failed,
        errors,
        connectors_created,
    })
}

async fn replace_doc_chunks(
    pool: &sqlx::SqlitePool,
    document_id: &str,
    chunks: &[context_harness::models::Chunk],
) -> Result<(), AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query(
        "DELETE FROM chunk_vectors WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
    )
    .bind(document_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query(
        "DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id = ?)",
    )
    .bind(document_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query("DELETE FROM chunks_fts WHERE document_id = ?")
        .bind(document_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    sqlx::query("DELETE FROM chunks WHERE document_id = ?")
        .bind(document_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    for chunk in chunks {
        sqlx::query(
            "INSERT INTO chunks (id, document_id, chunk_index, text, hash) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&chunk.id)
        .bind(&chunk.document_id)
        .bind(chunk.chunk_index)
        .bind(&chunk.text)
        .bind(&chunk.hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        sqlx::query("INSERT INTO chunks_fts (chunk_id, document_id, text) VALUES (?, ?, ?)")
            .bind(&chunk.id)
            .bind(&chunk.document_id)
            .bind(&chunk.text)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }

    tx.commit()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(())
}
