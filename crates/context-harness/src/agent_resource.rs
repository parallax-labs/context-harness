//! Standalone executable-agent declarations. Loading is explicit: existing MCP
//! prompt registries and unrelated commands do not discover these resources.

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::agents::TomlAgent;
use crate::config::{Config, ResolvedConfig};
use crate::ctx_dirs::{self, ConfigSourceKind};

/// A declarative model alias. Credentials are referenced, never read here.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefinition {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

impl ModelDefinition {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.provider.trim().is_empty(),
            "model provider is required"
        );
        ensure!(
            !self.model.trim().is_empty(),
            "provider model name is required"
        );
        if let Some(env) = &self.api_key_env {
            let mut chars = env.chars();
            ensure!(
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "api_key_env must be an environment variable name"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentResource {
    pub agent: AgentSettings,
    pub prompt: PromptSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSettings {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub model: String,
    #[serde(default)]
    pub tools: Vec<String>,
    /// Replaces a lower-precedence standalone definition as a whole.
    #[serde(default, rename = "override")]
    pub override_existing: bool,
    #[serde(default)]
    pub execution: ExecutionLimits,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default, skip_serializing_if = "DelegationSettings::is_empty")]
    pub delegation: DelegationSettings,
}

/// Explicit agent targets that the runtime may resolve for `agent.invoke`.
/// Empty settings are omitted from version snapshots for backwards compatibility.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DelegationSettings {
    pub allow: Vec<String>,
}

impl DelegationSettings {
    fn is_empty(&self) -> bool {
        self.allow.is_empty()
    }

    fn validate(&self, agent_name: &str, tools: &[String]) -> Result<()> {
        ensure!(
            self.allow.len() <= 32,
            "delegation allows at most 32 agents"
        );
        ensure!(
            self.allow.iter().all(|name| identifier(name)),
            "invalid delegation agent name"
        );
        ensure!(
            self.allow.iter().collect::<HashSet<_>>().len() == self.allow.len(),
            "duplicate delegation agent name"
        );
        ensure!(
            !self.allow.iter().any(|name| name == agent_name),
            "an agent cannot delegate to itself"
        );
        ensure!(
            tools.iter().any(|tool| tool == "agent.invoke") == !self.is_empty(),
            "agent.invoke and a nonempty delegation.allow must be declared together"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptSettings {
    pub system: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionLimits {
    pub max_turns: u32,
    pub timeout_seconds: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 12,
            timeout_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadOnly,
    AgentDelegate,
    WorkspaceWrite,
    ProcessExecute,
    Network,
    ExternalSideEffect,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    ReadOnly,
}

/// Omission means read-only. An explicit allow list replaces that default.
/// Capability enforcement belongs to the runtime, not prompt projection.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<PermissionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<Capability>>,
    pub require_approval: Vec<Capability>,
}

impl Permissions {
    pub fn allowed(&self) -> Vec<Capability> {
        self.allow
            .clone()
            .unwrap_or_else(|| vec![Capability::ReadOnly])
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.mode.is_none() || (self.allow.is_none() && self.require_approval.is_empty()),
            "permissions.mode cannot be combined with allow or require_approval"
        );
        let allowed = self.allowed();
        ensure!(
            allowed.iter().collect::<HashSet<_>>().len() == allowed.len(),
            "duplicate allowed capability"
        );
        ensure!(
            self.require_approval.iter().collect::<HashSet<_>>().len()
                == self.require_approval.len(),
            "duplicate approval capability"
        );
        ensure!(
            !self.require_approval.iter().any(|c| allowed.contains(c)),
            "a capability cannot be both allowed and require approval"
        );
        Ok(())
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl AgentResource {
    pub fn parse(content: &str) -> Result<Self> {
        let resource: Self = toml::from_str(content).context("invalid agent resource TOML")?;
        ensure!(identifier(&resource.agent.name), "invalid agent name");
        ensure!(identifier(&resource.agent.model), "invalid model alias");
        ensure!(
            !resource.prompt.system.trim().is_empty(),
            "prompt.system is required"
        );
        ensure!(
            resource.agent.execution.max_turns > 0,
            "max_turns must be greater than zero"
        );
        ensure!(
            resource.agent.execution.timeout_seconds > 0,
            "timeout_seconds must be greater than zero"
        );
        let tools = &resource.agent.tools;
        ensure!(tools.iter().all(|t| identifier(t)), "invalid tool name");
        ensure!(
            tools.iter().collect::<HashSet<_>>().len() == tools.len(),
            "duplicate tool name"
        );
        resource.agent.permissions.validate()?;
        resource
            .agent
            .delegation
            .validate(&resource.agent.name, tools)?;
        Ok(resource)
    }

    /// Hash parsed, default-expanded content; formatting/comments/path do not
    /// affect the version. Resolved model configuration is a separate snapshot.
    pub fn version(&self) -> Result<String> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(self)?)
        ))
    }

    /// Adapt the static prompt to the existing Agent trait without changing its
    /// contract. This does not grant runtime permissions to an external client.
    #[allow(dead_code)]
    pub fn prompt_agent(&self) -> TomlAgent {
        TomlAgent::new(
            self.agent.name.clone(),
            self.agent.description.clone(),
            self.agent.tools.clone(),
            self.prompt.system.clone(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceScope {
    Global,
    Workspace,
    Explicit,
}

#[derive(Debug, Clone)]
pub struct ResourceDirectory {
    pub path: PathBuf,
    pub scope: ResourceScope,
}

/// Choose resource paths using the same explicit-config isolation as settings.
/// The caller supplies the workspace root, never inferring it from a DB path.
pub fn resource_directories(
    resolved: &ResolvedConfig,
    root: &Path,
    global: &Path,
) -> Vec<ResourceDirectory> {
    if matches!(
        resolved.source,
        ConfigSourceKind::Explicit | ConfigSourceKind::Env
    ) {
        return resolved
            .path
            .as_ref()
            .map(|path| {
                let path = if path.is_absolute() {
                    path.clone()
                } else {
                    root.join(path)
                };
                ResourceDirectory {
                    path: path.parent().unwrap_or(root).join("agents"),
                    scope: ResourceScope::Explicit,
                }
            })
            .into_iter()
            .collect();
    }
    vec![
        ResourceDirectory {
            path: global.join("agents"),
            scope: ResourceScope::Global,
        },
        ResourceDirectory {
            path: root.join(".ctx/agents"),
            scope: ResourceScope::Workspace,
        },
    ]
}

pub fn cli_resource_directories(resolved: &ResolvedConfig) -> Result<Vec<ResourceDirectory>> {
    Ok(resource_directories(
        resolved,
        &std::env::current_dir()?,
        &ctx_dirs::config_dir(),
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadedAgentResource {
    pub path: PathBuf,
    pub scope: ResourceScope,
    pub version: String,
    pub definition: AgentResource,
}

/// Read each layer in sorted order and replace whole resources only with an
/// explicit override. A duplicate name within one layer is always an error.
pub fn load_resources(
    directories: &[ResourceDirectory],
    config: &Config,
) -> Result<BTreeMap<String, LoadedAgentResource>> {
    let mut resources: BTreeMap<String, LoadedAgentResource> = BTreeMap::new();
    let mut visited = HashSet::new();
    for directory in directories {
        let canonical = match directory.path.canonicalize() {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("reading {}", directory.path.display()))
            }
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }
        let mut paths = std::fs::read_dir(&canonical)
            .with_context(|| format!("reading {}", canonical.display()))?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        let mut names = HashSet::new();
        for path in paths
            .into_iter()
            .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
        {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let definition = AgentResource::parse(&content)
                .with_context(|| format!("agent resource {}", path.display()))?;
            let name = &definition.agent.name;
            ensure!(
                names.insert(name.clone()),
                "duplicate agent '{name}' in {}",
                canonical.display()
            );
            ensure!(!config.agents.inline.contains_key(name) && !config.agents.script.contains_key(name),
                "agent resource {} conflicts with legacy agent '{name}'; rename or migrate the legacy definition", path.display());
            if let Some(previous) = resources.get(name) {
                ensure!(
                    definition.agent.override_existing,
                    "agent '{name}' in {} shadows {}; set agent.override = true to replace it",
                    path.display(),
                    previous.path.display()
                );
            }
            let version = definition.version()?;
            resources.insert(
                name.clone(),
                LoadedAgentResource {
                    path,
                    scope: directory.scope,
                    version,
                    definition,
                },
            );
        }
    }
    // Validate references after replacement: a global agent overridden locally
    // may legitimately refer to a model alias absent from the effective config.
    for loaded in resources.values() {
        let alias = &loaded.definition.agent.model;
        let model = config.models.get(alias).with_context(|| {
            format!(
                "agent resource {} references unknown model '{alias}'",
                loaded.path.display()
            )
        })?;
        model
            .validate()
            .with_context(|| format!("model '{alias}' for {}", loaded.path.display()))?;
    }
    Ok(resources)
}

/// Combined discovery view. Legacy agents remain prompt-only and need no model.
#[derive(Debug, Serialize)]
pub struct AgentEntry {
    pub name: String,
    pub description: String,
    pub source: String,
    pub tools: Vec<String>,
    pub arguments: Vec<crate::agents::AgentArgument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<LoadedAgentResource>,
}

pub fn catalog(
    config: &Config,
    directories: &[ResourceDirectory],
) -> Result<BTreeMap<String, AgentEntry>> {
    let resources = load_resources(directories, config)?;
    let mut entries = BTreeMap::new();
    for (name, agent) in &config.agents.inline {
        entries.insert(
            name.clone(),
            AgentEntry {
                name: name.clone(),
                description: agent.description.clone(),
                source: "toml".into(),
                tools: agent.tools.clone(),
                arguments: vec![],
                system_prompt: Some(agent.system_prompt.clone()),
                resource: None,
            },
        );
    }
    for agent in crate::agent_script::load_agent_definitions(config)? {
        ensure!(
            !entries.contains_key(&agent.name),
            "ambiguous legacy agent '{}' (inline and Lua)",
            agent.name
        );
        entries.insert(
            agent.name.clone(),
            AgentEntry {
                name: agent.name,
                description: agent.description,
                source: "lua".into(),
                tools: agent.tools,
                arguments: agent.arguments,
                system_prompt: None,
                resource: None,
            },
        );
    }
    for (name, resource) in resources {
        let agent = &resource.definition.agent;
        entries.insert(
            name.clone(),
            AgentEntry {
                name,
                description: agent.description.clone(),
                source: "resource".into(),
                tools: agent.tools.clone(),
                arguments: vec![],
                system_prompt: Some(resource.definition.prompt.system.clone()),
                resource: Some(resource),
            },
        );
    }
    Ok(entries)
}

pub fn list(config: &Config, directories: &[ResourceDirectory], json_output: bool) -> Result<()> {
    let entries = catalog(config, directories)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries.values().collect::<Vec<_>>())?
        );
    } else {
        println!("{:<24} {:<10} {:<44} TOOLS", "AGENT", "TYPE", "DESCRIPTION");
        for entry in entries.values() {
            println!(
                "{:<24} {:<10} {:<44} {}",
                entry.name,
                entry.source,
                entry.description,
                entry.tools.join(", ")
            );
        }
        if entries.is_empty() {
            println!("No agents configured.");
        }
    }
    Ok(())
}

pub fn show(
    config: &Config,
    directories: &[ResourceDirectory],
    name: &str,
    json_output: bool,
) -> Result<()> {
    let mut entries = catalog(config, directories)?;
    let entry = entries
        .remove(name)
        .with_context(|| format!("agent '{name}' not found"))?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&entry)?);
    } else {
        println!(
            "Agent: {}\nSource: {}\nDescription: {}\nTools: {}",
            entry.name,
            entry.source,
            entry.description,
            entry.tools.join(", ")
        );
        if let Some(resource) = &entry.resource {
            println!("Path: {}\nScope: {:?}\nVersion: {}\nModel: {}\nMax turns: {}\nTimeout: {}s\nAllowed capabilities: {}\nApproval required: {}",
                resource.path.display(), resource.scope, resource.version, resource.definition.agent.model,
                resource.definition.agent.execution.max_turns, resource.definition.agent.execution.timeout_seconds,
                serde_json::to_string(&resource.definition.agent.permissions.allowed())?,
                serde_json::to_string(&resource.definition.agent.permissions.require_approval)?);
        }
        if let Some(prompt) = entry.system_prompt {
            println!("\nSystem prompt:\n{prompt}");
        }
        for argument in entry.arguments {
            println!(
                "Argument: {} (required: {}) — {}",
                argument.name, argument.required, argument.description
            );
        }
    }
    Ok(())
}

pub fn validate(config: &Config, directories: &[ResourceDirectory]) -> Result<()> {
    for (alias, model) in &config.models {
        ensure!(identifier(alias), "invalid model alias '{alias}'");
        model
            .validate()
            .with_context(|| format!("model '{alias}'"))?;
    }
    let entries = catalog(config, directories)?;
    println!(
        "Validated {} agents ({} standalone resources).",
        entries.len(),
        entries.values().filter(|e| e.resource.is_some()).count()
    );
    Ok(())
}

pub async fn test(
    config: &Config,
    directories: &[ResourceDirectory],
    name: &str,
    args: Vec<(String, String)>,
) -> Result<()> {
    // Preserve targeted legacy resolution: an unrelated broken Lua script or
    // resource must not prevent testing a configured legacy agent.
    if config.agents.script.contains_key(name) {
        return crate::agent_script::test_agent(name, args, config).await;
    }
    if !args.is_empty() {
        bail!("static agent '{name}' does not accept arguments");
    }
    if let Some(agent) = config.agents.inline.get(name) {
        println!(
            "Agent: {name}\nSource: toml\nTools: {}\n\nSystem prompt:\n{}",
            agent.tools.join(", "),
            agent.system_prompt
        );
    } else {
        let mut resources = load_resources(directories, config)?;
        let resource = resources
            .remove(name)
            .with_context(|| format!("agent '{name}' not found"))?;
        println!(
            "Agent: {name}\nSource: resource\nTools: {}\n\nSystem prompt:\n{}",
            resource.definition.agent.tools.join(", "),
            resource.definition.prompt.system
        );
    }
    Ok(())
}
