//! Standalone executable-agent declarations. Loading is explicit: existing MCP
//! prompt registries and unrelated commands do not discover these resources.

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{Config, ResolvedConfig};
use crate::ctx_dirs::{self, ConfigSourceKind};
use crate::profiles::TomlProfile;

/// A declarative model alias. Credentials are referenced, never read here.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefinition {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 12,
            timeout_seconds: 300,
            max_total_tokens: None,
            max_tool_calls: None,
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
        ensure!(
            resource.agent.execution.max_total_tokens != Some(0),
            "max_total_tokens must be greater than zero"
        );
        ensure!(
            resource.agent.execution.max_tool_calls != Some(0),
            "max_tool_calls must be greater than zero"
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

    /// Adapt the static prompt to the profile trait without changing its
    /// contract. This does not grant runtime permissions to an external client.
    pub fn prompt_profile(&self) -> TomlProfile {
        TomlProfile::new(
            self.agent.name.clone(),
            self.agent.description.clone(),
            self.agent.tools.clone(),
            self.prompt.system.clone(),
        )
    }

    /// Deprecated compatibility name for [`Self::prompt_profile`].
    #[allow(dead_code)]
    #[deprecated(note = "use prompt_profile")]
    pub fn prompt_agent(&self) -> TomlProfile {
        self.prompt_profile()
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
            ensure!(
                !config.profiles.inline.contains_key(name)
                    && !config.profiles.script.contains_key(name),
                "agent resource {} conflicts with profile '{name}'; rename one of the definitions",
                path.display()
            );
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

/// Combined profile discovery view. Profiles remain prompt-only and need no model.
#[derive(Debug, Serialize)]
pub struct ProfileEntry {
    pub name: String,
    pub description: String,
    pub source: String,
    pub tools: Vec<String>,
    pub arguments: Vec<crate::profiles::ProfileArgument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<LoadedAgentResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_config: Option<ModelInspection>,
}

/// Sanitized effective model settings for static inspection.
#[derive(Debug, Serialize)]
pub struct ModelInspection {
    pub alias: String,
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_only: Option<bool>,
}

pub fn catalog(
    config: &Config,
    directories: &[ResourceDirectory],
) -> Result<BTreeMap<String, ProfileEntry>> {
    let resources = load_resources(directories, config)?;
    let mut entries = BTreeMap::new();
    for (name, profile) in &config.profiles.inline {
        entries.insert(
            name.clone(),
            ProfileEntry {
                name: name.clone(),
                description: profile.description.clone(),
                source: "toml".into(),
                tools: profile.tools.clone(),
                arguments: vec![],
                system_prompt: Some(profile.system_prompt.clone()),
                resource: None,
                model_config: None,
            },
        );
    }
    for profile in crate::profile_script::load_profile_definitions(config)? {
        ensure!(
            !entries.contains_key(&profile.name),
            "ambiguous profile '{}' (inline and Lua)",
            profile.name
        );
        entries.insert(
            profile.name.clone(),
            ProfileEntry {
                name: profile.name,
                description: profile.description,
                source: "lua".into(),
                tools: profile.tools,
                arguments: profile.arguments,
                system_prompt: None,
                resource: None,
                model_config: None,
            },
        );
    }
    for (name, resource) in resources {
        let agent = &resource.definition.agent;
        let model_config = inspect_model(config, &agent.model)?;
        entries.insert(
            name.clone(),
            ProfileEntry {
                name,
                description: agent.description.clone(),
                source: "resource".into(),
                tools: agent.tools.clone(),
                arguments: vec![],
                system_prompt: Some(resource.definition.prompt.system.clone()),
                resource: Some(resource),
                model_config: Some(model_config),
            },
        );
    }
    Ok(entries)
}

pub fn list_profiles(
    config: &Config,
    directories: &[ResourceDirectory],
    json_output: bool,
) -> Result<()> {
    let entries = catalog(config, directories)?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries.values().collect::<Vec<_>>())?
        );
    } else {
        println!(
            "{:<24} {:<10} {:<44} TOOLS",
            "PROFILE", "TYPE", "DESCRIPTION"
        );
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
            println!("No profiles configured.");
        }
    }
    Ok(())
}

pub fn show_profile(
    config: &Config,
    directories: &[ResourceDirectory],
    name: &str,
    json_output: bool,
) -> Result<()> {
    let mut entries = catalog(config, directories)?;
    let entry = entries
        .remove(name)
        .with_context(|| format!("profile '{name}' not found"))?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&entry)?);
    } else {
        println!(
            "Profile: {}\nSource: {}\nDescription: {}\nTools: {}",
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

pub fn validate_profiles(config: &Config, directories: &[ResourceDirectory]) -> Result<()> {
    let providers = crate::agent_model::ModelProviderCatalog::with_builtins()?;
    for (alias, model) in &config.models {
        ensure!(identifier(alias), "invalid model alias '{alias}'");
        providers.validate_definition(alias, model)?;
    }
    let entries = catalog(config, directories)?;
    println!(
        "Validated {} profiles ({} projected from executable agents).",
        entries.len(),
        entries.values().filter(|e| e.resource.is_some()).count()
    );
    Ok(())
}

pub async fn test_profile(
    config: &Config,
    directories: &[ResourceDirectory],
    name: &str,
    args: Vec<(String, String)>,
) -> Result<()> {
    // Preserve targeted profile resolution: an unrelated broken Lua script or
    // resource must not prevent testing the selected profile.
    if config.profiles.script.contains_key(name) {
        return crate::profile_script::test_profile(name, args, config).await;
    }
    if !args.is_empty() {
        bail!("static profile '{name}' does not accept arguments");
    }
    if let Some(agent) = config.profiles.inline.get(name) {
        println!(
            "Profile: {name}\nSource: toml\nTools: {}\n\nSystem prompt:\n{}",
            agent.tools.join(", "),
            agent.system_prompt
        );
    } else {
        let mut resources = load_resources(directories, config)?;
        let resource = resources
            .remove(name)
            .with_context(|| format!("profile '{name}' not found"))?;
        println!(
            "Profile: {name}\nSource: agent projection\nTools: {}\n\nSystem prompt:\n{}",
            resource.definition.agent.tools.join(", "),
            resource.definition.prompt.system
        );
    }
    Ok(())
}

/// List executable standalone agents only. Prompt-only profiles are exposed by
/// the separate `ctx profile` command.
pub fn list(config: &Config, directories: &[ResourceDirectory], json_output: bool) -> Result<()> {
    let resources = resource_catalog(config, directories)?;
    validate_builtin_models(config, resources.values())?;
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&resources.values().collect::<Vec<_>>())?
        );
    } else {
        println!("{:<24} {:<44} MODEL", "AGENT", "DESCRIPTION");
        for entry in resources.values() {
            let resource = entry.resource.as_ref().expect("resource catalog entry");
            println!(
                "{:<24} {:<44} {}",
                entry.name, entry.description, resource.definition.agent.model
            );
        }
        if resources.is_empty() {
            println!("No executable agents configured.");
        }
    }
    Ok(())
}

/// Show one executable standalone agent.
pub fn show(
    config: &Config,
    directories: &[ResourceDirectory],
    name: &str,
    json_output: bool,
) -> Result<()> {
    let mut resources = resource_catalog(config, directories)?;
    let entry = resources
        .remove(name)
        .with_context(|| format!("executable agent '{name}' not found"))?;
    validate_builtin_models(config, std::iter::once(&entry))?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&entry)?);
    } else {
        let resource = entry.resource.as_ref().expect("resource catalog entry");
        println!(
            "Agent: {}\nDescription: {}\nPath: {}\nScope: {:?}\nVersion: {}\nModel: {}\nTools: {}\nMax turns: {}\nTimeout: {}s\nAllowed capabilities: {}\nApproval required: {}\n\nSystem prompt:\n{}",
            entry.name,
            entry.description,
            resource.path.display(),
            resource.scope,
            resource.version,
            resource.definition.agent.model,
            entry.tools.join(", "),
            resource.definition.agent.execution.max_turns,
            resource.definition.agent.execution.timeout_seconds,
            serde_json::to_string(&resource.definition.agent.permissions.allowed())?,
            serde_json::to_string(&resource.definition.agent.permissions.require_approval)?,
            entry.system_prompt.as_deref().unwrap_or_default(),
        );
        if let Some(model) = &entry.model_config {
            if model.local_only == Some(true) {
                println!(
                    "Model provider: {}\nProvider model: {}\nBase URL: {}\nProvider timeout: {}s\nLocal only: true",
                    model.provider,
                    model.model,
                    model.base_url.as_deref().expect("Ollama base URL"),
                    model.timeout_seconds.expect("Ollama timeout"),
                );
            }
        }
    }
    Ok(())
}

/// Validate executable agent resources and their model references.
pub fn validate(config: &Config, directories: &[ResourceDirectory]) -> Result<()> {
    let resources = load_resources(directories, config)?;
    let providers = crate::agent_model::ModelProviderCatalog::with_builtins()?;
    for resource in resources.values() {
        let alias = &resource.definition.agent.model;
        providers.validate_definition(
            alias,
            config.models.get(alias).context("model alias not found")?,
        )?;
    }
    println!("Validated {} executable agents.", resources.len());
    Ok(())
}

fn validate_builtin_models<'a>(
    config: &Config,
    entries: impl IntoIterator<Item = &'a ProfileEntry>,
) -> Result<()> {
    let providers = crate::agent_model::ModelProviderCatalog::with_builtins()?;
    for entry in entries {
        let resource = entry.resource.as_ref().expect("agent resource entry");
        let alias = &resource.definition.agent.model;
        providers.validate_definition(
            alias,
            config.models.get(alias).context("model alias not found")?,
        )?;
    }
    Ok(())
}

fn resource_catalog(
    config: &Config,
    directories: &[ResourceDirectory],
) -> Result<BTreeMap<String, ProfileEntry>> {
    Ok(load_resources(directories, config)?
        .into_iter()
        .map(|(name, resource)| {
            let agent = &resource.definition.agent;
            let model_config = inspect_model(config, &agent.model)
                .expect("loaded agent has a validated model definition");
            let entry = ProfileEntry {
                name: name.clone(),
                description: agent.description.clone(),
                source: "resource".into(),
                tools: agent.tools.clone(),
                arguments: vec![],
                system_prompt: Some(resource.definition.prompt.system.clone()),
                resource: Some(resource),
                model_config: Some(model_config),
            };
            (name, entry)
        })
        .collect())
}

fn inspect_model(config: &Config, alias: &str) -> Result<ModelInspection> {
    let definition = config
        .models
        .get(alias)
        .with_context(|| format!("unknown model alias '{alias}'"))?;
    let ollama = definition.provider == "ollama";
    let effective_ollama = ollama
        .then(|| crate::agent_model::ollama::effective_config(definition))
        .transpose()?;
    Ok(ModelInspection {
        alias: alias.into(),
        provider: definition.provider.clone(),
        model: definition.model.clone(),
        base_url: effective_ollama.as_ref().map(|(url, _)| url.clone()),
        timeout_seconds: effective_ollama.map(|(_, timeout)| timeout),
        local_only: ollama.then_some(true),
    })
}
