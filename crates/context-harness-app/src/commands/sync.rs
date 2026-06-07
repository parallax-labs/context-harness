use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, State};

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgressEvent {
    pub operation_id: String,
    pub connector: String,
    pub phase: String,
    pub current: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
    pub elapsed_ms: u64,
    pub message: Option<String>,
}

struct TauriProgressReporter {
    app_handle: tauri::AppHandle,
    operation_id: String,
    start: std::time::Instant,
}

impl context_harness::progress::SyncProgressReporter for TauriProgressReporter {
    fn report(&self, event: context_harness::progress::SyncProgressEvent) {
        let elapsed = self.start.elapsed().as_millis() as u64;
        let tauri_event = match event {
            context_harness::progress::SyncProgressEvent::Discovering { connector } => {
                SyncProgressEvent {
                    operation_id: self.operation_id.clone(),
                    connector: connector.clone(),
                    phase: "discovering".to_string(),
                    current: 0,
                    total: None,
                    current_item: None,
                    elapsed_ms: elapsed,
                    message: Some(format!("Discovering files in {connector}...")),
                }
            }
            context_harness::progress::SyncProgressEvent::Ingesting {
                connector,
                n,
                total,
            } => SyncProgressEvent {
                operation_id: self.operation_id.clone(),
                connector: connector.clone(),
                phase: "ingesting".to_string(),
                current: n,
                total: Some(total),
                current_item: None,
                elapsed_ms: elapsed,
                message: Some(format!("Ingesting {n} / {total} documents from {connector}")),
            },
        };
        let _ = self.app_handle.emit("sync-progress", tauri_event);
    }
}

#[tauri::command]
pub async fn sync_start(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    target: Option<String>,
) -> Result<String, AppError> {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let op_id = operation_id.clone();
    let connector_target = target.unwrap_or_else(|| "all".to_string());

    let config = {
        let ws = state.workspace.read().await;
        let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;
        workspace.config.clone()
    };

    let app_handle = app.clone();
    let target_clone = connector_target.clone();

    tokio::spawn(async move {
        let start = std::time::Instant::now();

        let _ = app_handle.emit(
            "sync-progress",
            SyncProgressEvent {
                operation_id: op_id.clone(),
                connector: target_clone.clone(),
                phase: "scanning".to_string(),
                current: 0,
                total: None,
                current_item: None,
                elapsed_ms: 0,
                message: Some(format!("Starting sync for {target_clone}...")),
            },
        );

        let reporter = Arc::new(TauriProgressReporter {
            app_handle: app_handle.clone(),
            operation_id: op_id.clone(),
            start,
        });

        let result = context_harness::ingest::run_sync(
            &config,
            &target_clone,
            false,
            false,
            None,
            None,
            None,
            Some(reporter.as_ref()),
        )
        .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(()) => {
                let _ = app_handle.emit(
                    "sync-progress",
                    SyncProgressEvent {
                        operation_id: op_id,
                        connector: target_clone,
                        phase: "complete".to_string(),
                        current: 0,
                        total: None,
                        current_item: None,
                        elapsed_ms: elapsed,
                        message: Some(format!(
                            "Sync completed in {:.1}s",
                            elapsed as f64 / 1000.0
                        )),
                    },
                );
            }
            Err(e) => {
                let _ = app_handle.emit(
                    "sync-progress",
                    SyncProgressEvent {
                        operation_id: op_id,
                        connector: target_clone,
                        phase: "error".to_string(),
                        current: 0,
                        total: None,
                        current_item: None,
                        elapsed_ms: elapsed,
                        message: Some(format!("Sync failed: {e}")),
                    },
                );
            }
        }
    });

    Ok(operation_id)
}

#[tauri::command]
pub async fn sync_cancel(_operation_id: String) -> Result<(), AppError> {
    Ok(())
}
