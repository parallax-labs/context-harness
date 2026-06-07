use serde::Serialize;
use tauri::State;

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct ConnectorInfo {
    pub name: String,
    pub connector_type: String,
    pub document_count: u64,
    pub last_sync: Option<String>,
    pub healthy: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectorTestResult {
    pub success: bool,
    pub message: String,
}

#[tauri::command]
pub async fn connector_list(state: State<'_, AppState>) -> Result<Vec<ConnectorInfo>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let sources = context_harness::sources::get_sources(&workspace.config);
    let mut connectors = Vec::new();

    for src in &sources {
        let source_label = &src.name;
        let doc_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM documents WHERE source = ?",
        )
        .bind(source_label)
        .fetch_one(&workspace.pool)
        .await
        .unwrap_or(0);

        let last_sync: Option<String> =
            sqlx::query_scalar("SELECT updated_at FROM checkpoints WHERE source = ?")
                .bind(source_label)
                .fetch_optional(&workspace.pool)
                .await
                .ok()
                .flatten()
                .map(|ts: i64| {
                    chrono::DateTime::from_timestamp(ts, 0)
                        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                        .unwrap_or_else(|| ts.to_string())
                });

        let connector_type = if source_label.starts_with("filesystem:") {
            "filesystem"
        } else if source_label.starts_with("git:") {
            "git"
        } else if source_label.starts_with("s3:") {
            "s3"
        } else if source_label.starts_with("script:") {
            "script"
        } else {
            "unknown"
        };

        connectors.push(ConnectorInfo {
            name: source_label.clone(),
            connector_type: connector_type.to_string(),
            document_count: doc_count as u64,
            last_sync,
            healthy: src.healthy,
            notes: src.notes.clone(),
        });
    }

    Ok(connectors)
}

#[tauri::command]
pub async fn connector_add(
    state: State<'_, AppState>,
    connector_type: String,
    name: String,
    config: serde_json::Value,
) -> Result<(), AppError> {
    let mut ws = state.workspace.write().await;
    let workspace = ws.as_mut().ok_or(AppError::NoWorkspace)?;

    let config_path = workspace.path.join("config").join("ctx.toml");
    let mut contents = std::fs::read_to_string(&config_path)
        .map_err(|e| AppError::Internal(format!("Failed to read config: {e}")))?;

    let section = format!("\n[connectors.{connector_type}.{name}]\n");
    contents.push_str(&section);

    if let Some(obj) = config.as_object() {
        for (key, value) in obj {
            let val_str = match value {
                serde_json::Value::String(s) => format!("{key} = \"{s}\"\n"),
                serde_json::Value::Bool(b) => format!("{key} = {b}\n"),
                serde_json::Value::Number(n) => format!("{key} = {n}\n"),
                serde_json::Value::Array(arr) => {
                    let items: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| format!("\"{s}\"")))
                        .collect();
                    format!("{key} = [{}]\n", items.join(", "))
                }
                _ => continue,
            };
            contents.push_str(&val_str);
        }
    }

    std::fs::write(&config_path, &contents)
        .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;

    let new_config = super::load_and_resolve_config(&workspace.path, &config_path)
        .map_err(|e| AppError::InvalidConfig(e.to_string()))?;
    workspace.config = new_config;

    Ok(())
}

#[tauri::command]
pub async fn connector_update(
    state: State<'_, AppState>,
    connector_type: String,
    name: String,
    config: serde_json::Value,
) -> Result<(), AppError> {
    // For updates, we read the TOML, modify, and rewrite.
    // A simple approach: remove the old section and re-add.
    connector_remove(state.clone(), connector_type.clone(), name.clone(), false).await?;
    connector_add(state, connector_type, name, config).await
}

#[tauri::command]
pub async fn connector_remove(
    state: State<'_, AppState>,
    connector_type: String,
    name: String,
    purge_documents: bool,
) -> Result<(), AppError> {
    let mut ws = state.workspace.write().await;
    let workspace = ws.as_mut().ok_or(AppError::NoWorkspace)?;

    let config_path = workspace.path.join("config").join("ctx.toml");
    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| AppError::Internal(format!("Failed to read config: {e}")))?;

    let section_header = format!("[connectors.{connector_type}.{name}]");
    let lines: Vec<&str> = contents.lines().collect();
    let mut in_section = false;
    let mut filtered = Vec::new();

    for line in &lines {
        if line.trim() == section_header {
            in_section = true;
            continue;
        }
        if in_section && line.trim_start().starts_with('[') {
            in_section = false;
        }
        if !in_section {
            filtered.push(*line);
        }
    }

    let new_contents = filtered.join("\n");
    std::fs::write(&config_path, &new_contents)
        .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;

    if purge_documents {
        let source_label = format!("{connector_type}:{name}");
        sqlx::query("DELETE FROM documents WHERE source = ?")
            .bind(&source_label)
            .execute(&workspace.pool)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }

    let new_config = super::load_and_resolve_config(&workspace.path, &config_path)
        .map_err(|e| AppError::InvalidConfig(e.to_string()))?;
    workspace.config = new_config;

    Ok(())
}

#[tauri::command]
pub async fn connector_test(
    state: State<'_, AppState>,
    connector_type: String,
    name: String,
) -> Result<ConnectorTestResult, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let sources = context_harness::sources::get_sources(&workspace.config);
    let source_label = format!("{connector_type}:{name}");

    let status = sources.iter().find(|s| s.name == source_label);

    match status {
        Some(s) => Ok(ConnectorTestResult {
            success: s.healthy,
            message: s
                .notes
                .clone()
                .unwrap_or_else(|| "Connector is healthy".to_string()),
        }),
        None => Ok(ConnectorTestResult {
            success: false,
            message: format!("Connector '{source_label}' not found in configuration"),
        }),
    }
}
