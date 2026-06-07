use serde::Serialize;
use tauri::State;

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct RegistryInfo {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
    pub readonly: bool,
    pub is_git: bool,
    pub installed: bool,
    pub connectors: usize,
    pub tools: usize,
    pub agents: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryExtension {
    pub name: String,
    pub description: String,
    pub extension_type: String,
    pub registry: String,
    pub tags: Vec<String>,
    pub required_config: Vec<String>,
}

#[tauri::command]
pub async fn registry_status(
    state: State<'_, AppState>,
) -> Result<Vec<RegistryInfo>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let mgr = context_harness::registry::RegistryManager::from_config(&workspace.config);
    let infos: Vec<RegistryInfo> = mgr
        .registries()
        .into_iter()
        .map(|r| {
            let url = workspace
                .config
                .registries
                .get(&r.name)
                .and_then(|c| c.url.clone());
            RegistryInfo {
                name: r.name,
                path: r.path.display().to_string(),
                url,
                readonly: r.readonly,
                is_git: r.is_git,
                installed: true,
                connectors: r.connectors,
                tools: r.tools,
                agents: r.agents,
            }
        })
        .collect();

    // Also include configured-but-not-installed registries
    let installed_names: Vec<String> = infos.iter().map(|i| i.name.clone()).collect();
    let mut result = infos;
    for (name, cfg) in &workspace.config.registries {
        if !installed_names.contains(name) {
            result.push(RegistryInfo {
                name: name.clone(),
                path: cfg.path.display().to_string(),
                url: cfg.url.clone(),
                readonly: cfg.readonly,
                is_git: false,
                installed: false,
                connectors: 0,
                tools: 0,
                agents: 0,
            });
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn registry_list_extensions(
    state: State<'_, AppState>,
    extension_type: Option<String>,
) -> Result<Vec<RegistryExtension>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let mgr = context_harness::registry::RegistryManager::from_config(&workspace.config);
    let all = mgr.list_all();

    let extensions: Vec<RegistryExtension> = all
        .into_iter()
        .filter(|e| {
            if let Some(ref filter) = extension_type {
                let kind_plural = format!("{}s", e.kind);
                &kind_plural == filter || &e.kind == filter
            } else {
                true
            }
        })
        .map(|e| RegistryExtension {
            name: e.name,
            description: e.entry.description,
            extension_type: e.kind,
            registry: e.registry_name,
            tags: e.entry.tags,
            required_config: e.entry.required_config,
        })
        .collect();

    Ok(extensions)
}

#[tauri::command]
pub async fn registry_search(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<RegistryExtension>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let mgr = context_harness::registry::RegistryManager::from_config(&workspace.config);
    let all = mgr.list_all();

    let query_lower = query.to_lowercase();
    let results: Vec<RegistryExtension> = all
        .into_iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&query_lower)
                || e.entry.description.to_lowercase().contains(&query_lower)
                || e.entry
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&query_lower))
        })
        .map(|e| RegistryExtension {
            name: e.name,
            description: e.entry.description,
            extension_type: e.kind,
            registry: e.registry_name,
            tags: e.entry.tags,
            required_config: e.entry.required_config,
        })
        .collect();

    Ok(results)
}

#[tauri::command]
pub async fn registry_init(
    state: State<'_, AppState>,
    url: Option<String>,
    name: Option<String>,
) -> Result<RegistryInfo, AppError> {
    let mut ws = state.workspace.write().await;
    let workspace = ws.as_mut().ok_or(AppError::NoWorkspace)?;

    let registry_url = url.unwrap_or_else(|| {
        "https://github.com/parallax-labs/ctx-registry.git".to_string()
    });
    let registry_name = name.unwrap_or_else(|| "community".to_string());

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let target_dir = std::path::PathBuf::from(&home)
        .join(".ctx")
        .join("registries")
        .join(&registry_name);

    if !target_dir.exists() {
        context_harness::registry::clone_registry(&registry_url, Some("main"), &target_dir)
            .map_err(|e| AppError::Internal(format!("Failed to clone registry: {e}")))?;
    }

    let manifest = context_harness::registry::load_manifest(&target_dir)
        .map_err(|e| AppError::Internal(format!("Failed to load manifest: {e}")))?;

    // Add registry to workspace config file
    let config_path = workspace.path.join("config").join("ctx.toml");
    if !workspace.config.registries.contains_key(&registry_name) {
        let section = format!(
            "\n[registries.{}]\nurl = \"{}\"\nbranch = \"main\"\npath = \"{}\"\nreadonly = true\nauto_update = true\n",
            registry_name,
            registry_url,
            target_dir.display()
        );
        let mut content = std::fs::read_to_string(&config_path)
            .map_err(|e| AppError::Internal(format!("Failed to read config: {e}")))?;
        content.push_str(&section);
        std::fs::write(&config_path, &content)
            .map_err(|e| AppError::Internal(format!("Failed to write config: {e}")))?;

        // Reload config
        let new_config = super::load_and_resolve_config(&workspace.path, &config_path)
            .map_err(|e| AppError::InvalidConfig(e.to_string()))?;
        workspace.config = new_config;
    }

    Ok(RegistryInfo {
        name: registry_name,
        path: target_dir.display().to_string(),
        url: Some(registry_url),
        readonly: true,
        is_git: true,
        installed: true,
        connectors: manifest.connectors.len(),
        tools: manifest.tools.len(),
        agents: manifest.agents.len(),
    })
}

#[tauri::command]
pub async fn registry_update(
    state: State<'_, AppState>,
    registry_name: Option<String>,
) -> Result<String, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let mut updated = 0u32;
    let mut messages = Vec::new();

    for (name, cfg) in &workspace.config.registries {
        if let Some(ref filter) = registry_name {
            if name != filter {
                continue;
            }
        }

        if !cfg.path.exists() {
            messages.push(format!("'{}': not installed", name));
            continue;
        }

        if !context_harness::registry::is_git_repo(&cfg.path) {
            messages.push(format!("'{}': not a git repository", name));
            continue;
        }

        match context_harness::registry::pull_registry(&cfg.path) {
            Ok(()) => {
                messages.push(format!("'{}': updated", name));
                updated += 1;
            }
            Err(e) => {
                messages.push(format!("'{}': failed — {}", name, e));
            }
        }
    }

    if messages.is_empty() {
        Ok("No registries configured".to_string())
    } else {
        Ok(format!("Updated {} registries. {}", updated, messages.join("; ")))
    }
}

#[tauri::command]
pub async fn registry_install(
    state: State<'_, AppState>,
    registry_name: String,
) -> Result<String, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let cfg = workspace
        .config
        .registries
        .get(&registry_name)
        .ok_or_else(|| AppError::Internal(format!("Registry '{}' not found in config", registry_name)))?;

    let url = cfg
        .url
        .as_ref()
        .ok_or_else(|| AppError::Internal(format!("Registry '{}' has no URL configured", registry_name)))?;

    if cfg.path.exists() {
        return Ok(format!("Registry '{}' already installed at {}", registry_name, cfg.path.display()));
    }

    context_harness::registry::clone_registry(url, cfg.branch.as_deref(), &cfg.path)
        .map_err(|e| AppError::Internal(format!("Failed to clone registry: {e}")))?;

    let manifest = context_harness::registry::load_manifest(&cfg.path);
    match manifest {
        Ok(m) => Ok(format!(
            "Installed '{}': {} connectors, {} tools, {} agents",
            registry_name,
            m.connectors.len(),
            m.tools.len(),
            m.agents.len()
        )),
        Err(_) => Ok(format!("Installed '{}' (no registry.toml found)", registry_name)),
    }
}

#[tauri::command]
pub async fn registry_add_extension(
    state: State<'_, AppState>,
    extension_type: String,
    extension_name: String,
) -> Result<String, AppError> {
    let mut ws = state.workspace.write().await;
    let workspace = ws.as_mut().ok_or(AppError::NoWorkspace)?;

    let extension_id = format!("{}s/{}", extension_type, extension_name);

    let mgr = context_harness::registry::RegistryManager::from_config(&workspace.config);
    let ext = mgr
        .resolve(&extension_id)
        .ok_or_else(|| AppError::Internal(format!("Extension '{}' not found", extension_id)))?;

    let config_path = workspace.path.join("config").join("ctx.toml");

    context_harness::registry::cmd_add(&workspace.config, &extension_id, &config_path)
        .map_err(|e| AppError::Internal(format!("Failed to add extension: {e}")))?;

    // Reload config
    let new_config = super::load_and_resolve_config(&workspace.path, &config_path)
        .map_err(|e| AppError::InvalidConfig(e.to_string()))?;
    workspace.config = new_config;

    let mut msg = format!("Added {}s/{} to workspace config", ext.kind, ext.name);
    if !ext.entry.required_config.is_empty() {
        msg.push_str(&format!(
            ". Edit config to set: {}",
            ext.entry.required_config.join(", ")
        ));
    }
    Ok(msg)
}
