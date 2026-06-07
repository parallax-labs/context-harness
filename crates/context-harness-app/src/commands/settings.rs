use tauri::{Manager, State};

use crate::state::{AppSettings, AppState};

use super::AppError;

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<AppSettings, AppError> {
    let settings = state.settings.read().await;
    Ok(settings.clone())
}

#[tauri::command]
pub async fn settings_update(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    settings: AppSettings,
) -> Result<(), AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))?;

    {
        let mut current = state.settings.write().await;
        *current = settings;
        crate::state::apply_env_keys(&current);
        crate::state::save_settings(&app_data_dir, &current)
            .map_err(|e| AppError::Internal(format!("Failed to save settings: {e}")))?;
    }

    Ok(())
}
