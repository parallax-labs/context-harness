//! Public assembly seam for trusted hosts that execute standalone agents.
//!
//! Builder configuration is inert. [`AgentHostBuilder::build`] initializes the
//! existing runtime store and binds explicitly supplied implementations, but it
//! does not call a model, execute a tool, resolve credentials, or start workers.

use crate::{
    agent_model::{ModelProviderCatalog, ModelRegistry},
    agent_resource::{load_resources, LoadedAgentResource, ResourceDirectory},
    agent_runtime::{
        policy::{ApprovalHandler, DenyApprovals, RuntimePolicy},
        static_tool_identity, workspace_id, AgentRuntime,
    },
    agent_store::{AgentRun, AgentRunStore},
    agent_task_store::{
        AcceptedTaskIdentity, AgentTaskStore, AgentTaskSubmission, AgentTaskSubmissionResult,
        DEFAULT_QUEUE_LIMIT,
    },
    app_store::SqliteAppStore,
    config::Config,
    tool_binding::{self, HostToolAuthority, ToolImplementationCatalog, CATALOG_CONTRACT_VERSION},
};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path, path::PathBuf, sync::Arc};
use tokio::sync::watch;

struct ToolBindings {
    directories: Vec<ResourceDirectory>,
    catalog: ToolImplementationCatalog,
    authority: Arc<HostToolAuthority>,
}

/// Trusted inputs for assembling one direct-execution agent host.
pub struct AgentHostBuilder {
    config: Config,
    root: PathBuf,
    models: ModelProviderCatalog,
    agent_directories: Vec<ResourceDirectory>,
    tool_bindings: Option<ToolBindings>,
    policy: RuntimePolicy,
    approvals: Arc<dyn ApprovalHandler>,
}

impl AgentHostBuilder {
    pub fn new(config: Config, root: &Path, models: ModelProviderCatalog) -> Result<Self> {
        Ok(Self {
            config,
            root: root
                .canonicalize()
                .context("canonicalizing agent-host workspace root")?,
            models,
            agent_directories: Vec::new(),
            tool_bindings: None,
            policy: RuntimePolicy {
                allow: Vec::new(),
                require_approval: Vec::new(),
            },
            approvals: Arc::new(DenyApprovals),
        })
    }

    pub fn with_agent_resources(mut self, directories: Vec<ResourceDirectory>) -> Self {
        self.agent_directories = directories;
        self
    }

    /// Install trusted tool factories and explicit host-issued authority.
    pub fn with_tool_bindings(
        mut self,
        directories: Vec<ResourceDirectory>,
        catalog: ToolImplementationCatalog,
        authority: Arc<HostToolAuthority>,
    ) -> Self {
        self.tool_bindings = Some(ToolBindings {
            directories,
            catalog,
            authority,
        });
        self
    }

    /// Set the host permission ceiling and approval handler.
    ///
    /// The builder grants no capabilities unless this method is called.
    pub fn with_policy(
        mut self,
        policy: RuntimePolicy,
        approvals: Arc<dyn ApprovalHandler>,
    ) -> Self {
        self.policy = policy;
        self.approvals = approvals;
        self
    }

    /// Assemble the existing direct runtime for one agent and its delegation graph.
    pub async fn build(self, agent_name: &str) -> Result<AgentHost> {
        ensure!(!agent_name.trim().is_empty(), "agent name is required");
        let resources = load_resources(&self.agent_directories, &self.config)?;
        let resource = resources
            .get(agent_name)
            .cloned()
            .context("standalone agent not found; profiles are prompt-only")?;
        let definitions = reachable_model_definitions(&self.config, &resources, agent_name)?;
        let models = ModelRegistry::from_config_with_catalog(&definitions, &self.models)?;
        let mut runtime = AgentRuntime::new(self.config, &self.root, models)
            .await?
            .with_resources(resources)
            .with_policy(self.policy, self.approvals);
        if let Some(bindings) = self.tool_bindings {
            runtime = runtime
                .with_tool_binding_catalog(
                    &bindings.directories,
                    &bindings.catalog,
                    bindings.authority,
                )
                .await?;
        }
        Ok(AgentHost { runtime, resource })
    }

    /// Resolve and persist accepted task intent without constructing providers,
    /// binding tools, resolving secrets, or invoking runtime behavior.
    pub async fn build_task_submitter(self, agent_name: &str) -> Result<AgentTaskSubmitter> {
        let resolved = self.resolve_task_identity(agent_name)?;
        let mut config = self.config;
        if config.db.path.is_relative() {
            config.db.path = self.root.join(&config.db.path);
        }
        SqliteAppStore::initialize_config(&config).await?;
        let app = SqliteAppStore::connect(&config).await?;
        let store = app.agent_tasks(&resolved.workspace_id)?;
        Ok(AgentTaskSubmitter {
            store,
            agent_name: resolved.agent_name,
            agent_version: resolved.agent_version,
            accepted_identity: resolved.accepted_identity,
        })
    }

    /// Compute the immutable, non-secret identity accepted by a task. This
    /// static path does not open the application database.
    pub fn resolve_task_identity(&self, agent_name: &str) -> Result<ResolvedTaskIdentity> {
        ensure!(!agent_name.trim().is_empty(), "agent name is required");
        let resources = load_resources(&self.agent_directories, &self.config)?;
        let resource = resources
            .get(agent_name)
            .context("standalone agent not found; profiles are prompt-only")?;
        let reachable = reachable_agent_resources(&resources, agent_name)?;
        let definitions = reachable_model_definitions(&self.config, &resources, agent_name)?;
        self.models.validate_config(&definitions)?;

        let mut models = BTreeMap::new();
        for (alias, definition) in &definitions {
            let implementation = self.models.implementation(&definition.provider)?;
            models.insert(
                alias.clone(),
                serde_json::json!({
                    "provider": definition.provider,
                    "model": definition.model,
                    "implementation_id": implementation.id(),
                    "implementation_version": implementation.version(),
                }),
            );
        }

        let mut resolved_bindings = BTreeMap::new();
        if let Some(bindings) = &self.tool_bindings {
            let selected_names = reachable
                .values()
                .flat_map(|resource| resource.definition.agent.tools.iter())
                .collect::<std::collections::HashSet<_>>();
            let loaded = tool_binding::load_resources(&bindings.directories, &self.config)?
                .into_iter()
                .filter(|(name, _)| selected_names.contains(name))
                .collect();
            resolved_bindings = tool_binding::resolve_resources(loaded, &bindings.catalog)?;
        }
        let tools = static_tool_identity(
            &self.config,
            &self.root,
            &reachable,
            &resolved_bindings,
            &self.policy,
        )?;
        let agent_versions = reachable
            .iter()
            .map(|(name, resource)| (name.clone(), resource.version.clone()))
            .collect::<BTreeMap<_, _>>();
        let workspace_id = workspace_id(&self.root)?;
        let tool_authority = if let Some(bindings) = &self.tool_bindings {
            let mut paths = bindings
                .authority
                .paths()
                .iter()
                .map(|path| {
                    path.strip_prefix(&self.root)
                        .unwrap_or(path)
                        .as_os_str()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>();
            paths.sort();
            let mut sources = bindings.authority.sources().to_vec();
            sources.sort();
            Some(serde_json::json!({
                "capabilities": sorted_json_values(bindings.authority.capabilities())?,
                "paths": paths,
                "sources": sources,
            }))
        } else {
            None
        };
        let accepted_identity = AcceptedTaskIdentity::new(serde_json::json!({
            "schema_version": 1,
            "workspace_id": workspace_id,
            "root_agent": {"name": agent_name, "version": resource.version},
            "reachable_agents": agent_versions,
            "models": models,
            "tools": tools,
            "binding_contract_version": CATALOG_CONTRACT_VERSION,
            "host_policy": {
                "allow": sorted_json_values(&self.policy.allow)?,
                "require_approval": sorted_json_values(&self.policy.require_approval)?,
            },
            "tool_authority": tool_authority,
        }))?;
        Ok(ResolvedTaskIdentity {
            workspace_id,
            agent_name: agent_name.to_owned(),
            agent_version: resource.version.clone(),
            accepted_identity,
        })
    }
}

fn sorted_json_values<T: Serialize>(values: &[T]) -> Result<Vec<serde_json::Value>> {
    let mut values = values
        .iter()
        .map(serde_json::to_value)
        .collect::<serde_json::Result<Vec<_>>>()?;
    values.sort_by_key(|value| value.as_str().unwrap_or_default().to_owned());
    Ok(values)
}

pub struct ResolvedTaskIdentity {
    workspace_id: String,
    agent_name: String,
    agent_version: String,
    accepted_identity: AcceptedTaskIdentity,
}

impl ResolvedTaskIdentity {
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }
    pub fn agent_version(&self) -> &str {
        &self.agent_version
    }
    pub fn accepted_identity(&self) -> &AcceptedTaskIdentity {
        &self.accepted_identity
    }
}

/// Public, trusted submission surface for one statically resolved agent.
pub struct AgentTaskSubmitter {
    store: AgentTaskStore,
    agent_name: String,
    agent_version: String,
    accepted_identity: AcceptedTaskIdentity,
}

impl AgentTaskSubmitter {
    pub fn store(&self) -> &AgentTaskStore {
        &self.store
    }
    pub fn accepted_identity(&self) -> &AcceptedTaskIdentity {
        &self.accepted_identity
    }

    pub async fn submit(
        &self,
        request_key: impl Into<String>,
        input: impl Into<String>,
        payload_identity: Vec<u8>,
    ) -> Result<AgentTaskSubmissionResult> {
        self.submit_with_queue_limit(request_key, input, payload_identity, DEFAULT_QUEUE_LIMIT)
            .await
    }

    pub async fn submit_with_queue_limit(
        &self,
        request_key: impl Into<String>,
        input: impl Into<String>,
        payload_identity: Vec<u8>,
        queue_limit: u32,
    ) -> Result<AgentTaskSubmissionResult> {
        self.store
            .submit(AgentTaskSubmission {
                request_key: request_key.into(),
                agent_name: self.agent_name.clone(),
                agent_version: self.agent_version.clone(),
                input: input.into(),
                payload_identity,
                accepted_identity: self.accepted_identity.clone(),
                queue_limit,
            })
            .await
    }
}

/// A configured direct-execution host for one standalone agent.
pub struct AgentHost {
    runtime: AgentRuntime,
    resource: LoadedAgentResource,
}

impl AgentHost {
    pub fn resource(&self) -> &LoadedAgentResource {
        &self.resource
    }

    pub fn store(&self) -> &AgentRunStore {
        self.runtime.store()
    }

    pub async fn run(&self, input: &str, cancel: watch::Receiver<bool>) -> Result<AgentRun> {
        self.runtime.run(&self.resource, input, cancel).await
    }

    pub async fn resume(&self, id: &str, cancel: watch::Receiver<bool>) -> Result<AgentRun> {
        self.runtime.resume(id, &self.resource, cancel).await
    }
}

fn reachable_model_definitions(
    config: &Config,
    resources: &BTreeMap<String, LoadedAgentResource>,
    root: &str,
) -> Result<BTreeMap<String, crate::agent_resource::ModelDefinition>> {
    reachable_agent_resources(resources, root)?
        .into_iter()
        .map(|(name, resource)| {
            let alias = resource.definition.agent.model;
            let definition = config
                .models
                .get(&alias)
                .with_context(|| format!("model alias not found for agent '{name}'"))?
                .clone();
            Ok((alias, definition))
        })
        .collect()
}

fn reachable_agent_resources(
    resources: &BTreeMap<String, LoadedAgentResource>,
    root: &str,
) -> Result<BTreeMap<String, LoadedAgentResource>> {
    let mut names = vec![root.to_owned()];
    let mut visited = std::collections::HashSet::new();
    let mut reachable = BTreeMap::new();
    while let Some(name) = names.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        ensure!(
            visited.len() <= 256,
            "delegation catalog exceeds 256 reachable agents"
        );
        let resource = resources
            .get(&name)
            .context("delegation target not found")?;
        names.extend(resource.definition.agent.delegation.allow.iter().cloned());
        reachable.insert(name, resource.clone());
    }
    Ok(reachable)
}
