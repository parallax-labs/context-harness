pub mod agent;
pub mod connector;
pub mod document;
pub mod embedding;
pub mod registry;
pub mod search;
pub mod server;
pub mod settings;
pub mod sync;
pub mod workspace;

use std::path::Path;

use serde::Serialize;

/// Load a workspace config and resolve all relative paths against
/// the workspace directory so they work regardless of CWD.
pub fn load_and_resolve_config(
    workspace_dir: &Path,
    config_path: &Path,
) -> anyhow::Result<context_harness::config::Config> {
    let mut config = context_harness::config::load_config(config_path)?;

    // Resolve relative paths against workspace dir. For absolute paths
    // whose parent directory doesn't exist (e.g. Docker container paths
    // like /app/data/), fall back to workspace-relative resolution.
    let resolve = |p: &mut std::path::PathBuf| {
        if p.is_relative() {
            *p = workspace_dir.join(&*p);
        } else if let Some(parent) = p.parent() {
            if !parent.exists() {
                if let Ok(stripped) = p.strip_prefix("/") {
                    *p = workspace_dir.join(stripped);
                }
            }
        }
    };

    let resolve_dir = |p: &mut std::path::PathBuf| {
        if p.is_relative() {
            *p = workspace_dir.join(&*p);
        } else if !p.exists() {
            if let Ok(stripped) = p.strip_prefix("/") {
                *p = workspace_dir.join(stripped);
            }
        }
    };

    resolve(&mut config.db.path);
    for fc in config.connectors.filesystem.values_mut() {
        resolve_dir(&mut fc.root);
    }
    for sc in config.connectors.script.values_mut() {
        resolve(&mut sc.path);
    }
    for tc in config.tools.script.values_mut() {
        resolve(&mut tc.path);
    }
    for ac in config.agents.script.values_mut() {
        resolve(&mut ac.path);
    }
    for rc in config.registries.values_mut() {
        resolve_dir(&mut rc.path);
    }
    Ok(config)
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("No workspace is open")]
    NoWorkspace,
    #[error("Workspace not found: {0}")]
    WorkspaceNotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Connector error: {0}")]
    ConnectorError(String),
    #[error("Search error: {0}")]
    SearchError(String),
    #[error("Embedding error: {0}")]
    EmbeddingError(String),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
