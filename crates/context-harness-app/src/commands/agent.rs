use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentPromptResult {
    pub system: String,
    pub tools: Vec<String>,
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentMessage {
    pub role: String,
    pub content: String,
}

#[tauri::command]
pub async fn agent_list(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let mut agents: Vec<AgentInfo> = Vec::new();

    // Inline TOML agents
    for (name, cfg) in &workspace.config.agents.inline {
        agents.push(AgentInfo {
            name: name.clone(),
            description: cfg.description.clone(),
            tools: cfg.tools.clone(),
            source: "inline".to_string(),
        });
    }

    // Lua script agents
    match context_harness::agent_script::load_agent_definitions(&workspace.config) {
        Ok(defs) => {
            for def in defs {
                agents.push(AgentInfo {
                    name: def.name.clone(),
                    description: def.description.clone(),
                    tools: def.tools.clone(),
                    source: "script".to_string(),
                });
            }
        }
        Err(e) => {
            eprintln!("Warning: failed to load script agents: {e}");
        }
    }

    Ok(agents)
}

/// Build a full agent registry including both inline and script agents.
fn build_full_registry(
    config: &context_harness::config::Config,
) -> Result<context_harness::AgentRegistry, AppError> {
    let mut registry = context_harness::AgentRegistry::from_config(config)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if let Ok(defs) = context_harness::agent_script::load_agent_definitions(config) {
        let config_arc = Arc::new(config.clone());
        for def in defs {
            let adapter =
                context_harness::agent_script::LuaAgentAdapter::new(def, config_arc.clone());
            registry.register(Box::new(adapter));
        }
    }

    Ok(registry)
}

#[tauri::command]
pub async fn agent_get(
    state: State<'_, AppState>,
    name: String,
) -> Result<AgentInfo, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    // Check inline agents first
    if let Some(cfg) = workspace.config.agents.inline.get(&name) {
        return Ok(AgentInfo {
            name: name.clone(),
            description: cfg.description.clone(),
            tools: cfg.tools.clone(),
            source: "inline".to_string(),
        });
    }

    // Check script agents
    if let Ok(defs) = context_harness::agent_script::load_agent_definitions(&workspace.config) {
        if let Some(def) = defs.into_iter().find(|d| d.name == name) {
            return Ok(AgentInfo {
                name: def.name,
                description: def.description,
                tools: def.tools,
                source: "script".to_string(),
            });
        }
    }

    Err(AppError::Internal(format!("Agent '{name}' not found")))
}

#[tauri::command]
pub async fn agent_test(
    state: State<'_, AppState>,
    name: String,
    args: serde_json::Value,
) -> Result<AgentPromptResult, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let registry = build_full_registry(&workspace.config)?;

    let agent = registry
        .find(&name)
        .ok_or_else(|| AppError::Internal(format!("Agent '{name}' not found")))?;

    let tool_ctx = context_harness::ToolContext::new(Arc::new(workspace.config.clone()));
    let prompt = agent
        .resolve(args, &tool_ctx)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(AgentPromptResult {
        system: prompt.system,
        tools: prompt.tools,
        messages: prompt
            .messages
            .into_iter()
            .map(|m| AgentMessage {
                role: m.role,
                content: m.content,
            })
            .collect(),
    })
}
