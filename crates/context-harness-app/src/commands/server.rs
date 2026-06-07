use serde::Serialize;
use tauri::State;

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub running: bool,
    pub bind_address: Option<String>,
    pub uptime_secs: Option<u64>,
}

#[tauri::command]
pub async fn server_start(state: State<'_, AppState>) -> Result<(), AppError> {
    let ws = state.workspace.read().await;
    let _workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    // MVP: The MCP server lifecycle management requires keeping a
    // JoinHandle and shutdown signal. This is a placeholder that
    // indicates the feature's intended location.
    Err(AppError::ServerError(
        "MCP server management from the app is coming in Phase 2. \
         Use 'ctx serve mcp' from the CLI for now."
            .to_string(),
    ))
}

#[tauri::command]
pub async fn server_stop() -> Result<(), AppError> {
    Err(AppError::ServerError(
        "MCP server management from the app is coming in Phase 2.".to_string(),
    ))
}

#[tauri::command]
pub async fn server_status(state: State<'_, AppState>) -> Result<ServerStatus, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    Ok(ServerStatus {
        running: false,
        bind_address: Some(workspace.config.server.bind.clone()),
        uptime_secs: None,
    })
}
