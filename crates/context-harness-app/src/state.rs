use std::path::PathBuf;

use context_harness::config::Config;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub struct AppState {
    pub workspace: tokio::sync::RwLock<Option<WorkspaceState>>,
    pub settings: tokio::sync::RwLock<AppSettings>,
}

impl AppState {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            workspace: tokio::sync::RwLock::new(None),
            settings: tokio::sync::RwLock::new(settings),
        }
    }
}

pub struct WorkspaceState {
    pub config: Config,
    pub path: PathBuf,
    pub pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String,
    pub recent_workspaces: Vec<RecentWorkspace>,
    pub default_embedding_provider: String,
    pub auto_update: bool,
    #[serde(default)]
    pub openai_api_key: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            recent_workspaces: Vec::new(),
            default_embedding_provider: "local".to_string(),
            auto_update: true,
            openai_api_key: String::new(),
        }
    }
}

/// Apply API keys from settings as environment variables so the
/// context-harness library can pick them up at runtime.
pub fn apply_env_keys(settings: &AppSettings) {
    if !settings.openai_api_key.is_empty() {
        std::env::set_var("OPENAI_API_KEY", &settings.openai_api_key);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentWorkspace {
    pub name: String,
    pub path: String,
    pub last_opened: String,
}

/// Load settings from the app data directory, or return defaults.
pub fn load_settings(app_data_dir: &std::path::Path) -> AppSettings {
    let path = app_data_dir.join("settings.json");
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

/// Persist settings to the app data directory.
pub fn save_settings(
    app_data_dir: &std::path::Path,
    settings: &AppSettings,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(app_data_dir)?;
    let path = app_data_dir.join("settings.json");
    let json = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, json)?;
    Ok(())
}
