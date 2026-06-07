use std::path::PathBuf;

use serde::Serialize;
use tauri::{Manager, State};

use crate::state::{AppState, RecentWorkspace, WorkspaceState};

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub name: String,
    pub path: String,
    pub document_count: u64,
    pub chunk_count: u64,
    pub embedded_chunk_count: u64,
    pub last_sync: Option<String>,
    pub server_running: bool,
}

async fn query_workspace_info(
    pool: &sqlx::SqlitePool,
    name: &str,
    path: &str,
) -> Result<WorkspaceInfo, AppError> {
    let doc_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let chunk_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let embedded_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_vectors")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let last_sync: Option<String> = sqlx::query_scalar(
        "SELECT MAX(updated_at) FROM checkpoints",
    )
    .fetch_one(pool)
    .await
    .ok()
    .flatten()
    .map(|ts: i64| {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_else(|| ts.to_string())
    });

    Ok(WorkspaceInfo {
        name: name.to_string(),
        path: path.to_string(),
        document_count: doc_count as u64,
        chunk_count: chunk_count as u64,
        embedded_chunk_count: embedded_count as u64,
        last_sync,
        server_running: false,
    })
}

#[tauri::command]
pub async fn workspace_create(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    _name: String,
    path: String,
    embedding_provider: Option<String>,
) -> Result<WorkspaceInfo, AppError> {
    let workspace_dir = PathBuf::from(&path);
    std::fs::create_dir_all(&workspace_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create directory: {e}")))?;

    let config_dir = workspace_dir.join("config");
    std::fs::create_dir_all(&config_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create config dir: {e}")))?;

    let provider = embedding_provider.as_deref().unwrap_or("local");
    let ws_root = workspace_dir.to_string_lossy();
    let config_content = format!(
        r#"[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700

[embedding]
provider = "{provider}"

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem.workspace]
root = "{ws_root}"
include_globs = ["**/*.md", "**/*.txt", "**/*.pdf", "**/*.docx", "**/*.pptx", "**/*.xlsx", "**/*.rs", "**/*.ts", "**/*.js", "**/*.py", "**/*.go", "**/*.java", "**/*.yaml", "**/*.yml", "**/*.json", "**/*.toml", "**/*.sh"]
exclude_globs = ["config/**", "data/**"]
"#
    );

    let config_path = config_dir.join("ctx.toml");
    std::fs::write(&config_path, &config_content)
        .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;

    workspace_open(state, app, path).await
}

#[tauri::command]
pub async fn workspace_open(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    path: String,
) -> Result<WorkspaceInfo, AppError> {
    let workspace_dir = PathBuf::from(&path);

    if !workspace_dir.exists() {
        return Err(AppError::WorkspaceNotFound(format!(
            "Directory does not exist: {path}"
        )));
    }

    let config_dir = workspace_dir.join("config");
    let config_path = config_dir.join("ctx.toml");

    if !config_path.exists() {
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| AppError::Internal(format!("Failed to create config dir: {e}")))?;
        std::fs::create_dir_all(workspace_dir.join("data"))
            .map_err(|e| AppError::Internal(format!("Failed to create data dir: {e}")))?;

        let ws_root = workspace_dir.to_string_lossy();
        let config_content = format!(
            r#"[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700

[embedding]
provider = "local"

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem.workspace]
root = "{ws_root}"
include_globs = ["**/*.md", "**/*.txt", "**/*.pdf", "**/*.docx", "**/*.pptx", "**/*.xlsx", "**/*.rs", "**/*.ts", "**/*.js", "**/*.py", "**/*.go", "**/*.java", "**/*.yaml", "**/*.yml", "**/*.json", "**/*.toml", "**/*.sh"]
exclude_globs = ["config/**", "data/**"]
"#
        );

        std::fs::write(&config_path, &config_content)
            .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;
    }

    let config = super::load_and_resolve_config(&workspace_dir, &config_path)
        .map_err(|e| AppError::InvalidConfig(e.to_string()))?;

    context_harness::migrate::run_migrations(&config)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run migrations: {e}")))?;

    let pool = context_harness::db::connect(&config)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to database: {e}")))?;

    let workspace_name = workspace_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Workspace".to_string());

    let info = query_workspace_info(&pool, &workspace_name, &path).await?;

    {
        let mut ws = state.workspace.write().await;
        *ws = Some(WorkspaceState {
            config,
            path: workspace_dir.clone(),
            pool,
        });
    }

    // Update recent workspaces
    let app_data_dir = app.path().app_data_dir().ok();
    {
        let mut settings = state.settings.write().await;
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        settings
            .recent_workspaces
            .retain(|w| w.path != path);
        settings.recent_workspaces.insert(
            0,
            RecentWorkspace {
                name: workspace_name.clone(),
                path: path.clone(),
                last_opened: now,
            },
        );
        if settings.recent_workspaces.len() > 10 {
            settings.recent_workspaces.truncate(10);
        }
        if let Some(dir) = &app_data_dir {
            let _ = crate::state::save_settings(dir, &settings);
        }
    }

    Ok(info)
}

#[tauri::command]
pub async fn workspace_close(state: State<'_, AppState>) -> Result<(), AppError> {
    let mut ws = state.workspace.write().await;
    if let Some(workspace) = ws.take() {
        workspace.pool.close().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn workspace_get_info(state: State<'_, AppState>) -> Result<WorkspaceInfo, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let name = workspace
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Workspace".to_string());

    query_workspace_info(&workspace.pool, &name, &workspace.path.to_string_lossy()).await
}

#[tauri::command]
pub async fn workspace_list_recent(
    state: State<'_, AppState>,
) -> Result<Vec<RecentWorkspace>, AppError> {
    let settings = state.settings.read().await;
    Ok(settings.recent_workspaces.clone())
}

#[tauri::command]
pub async fn workspace_get_config(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;
    let config_path = workspace.path.join("config").join("ctx.toml");
    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| AppError::Internal(format!("Failed to read config: {e}")))?;
    Ok(serde_json::json!({ "raw": contents }))
}

#[tauri::command]
pub async fn workspace_update_config(
    state: State<'_, AppState>,
    raw: String,
) -> Result<(), AppError> {
    let mut ws = state.workspace.write().await;
    let workspace = ws.as_mut().ok_or(AppError::NoWorkspace)?;

    let config_path = workspace.path.join("config").join("ctx.toml");

    // Validate the TOML before writing
    toml::from_str::<toml::Value>(&raw)
        .map_err(|e| AppError::InvalidConfig(format!("Invalid TOML: {e}")))?;

    std::fs::write(&config_path, &raw)
        .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;

    let new_config = super::load_and_resolve_config(&workspace.path, &config_path)
        .map_err(|e| AppError::InvalidConfig(e.to_string()))?;

    workspace.config = new_config;
    Ok(())
}

#[tauri::command]
pub async fn workspace_export_config(
    state: State<'_, AppState>,
    destination: String,
) -> Result<(), AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;
    let config_path = workspace.path.join("config").join("ctx.toml");
    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| AppError::Internal(format!("Failed to read config: {e}")))?;
    std::fs::write(&destination, &contents)
        .map_err(|e| AppError::Internal(format!("Failed to export config: {e}")))?;
    Ok(())
}
