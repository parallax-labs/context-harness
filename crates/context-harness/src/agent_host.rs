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
        AgentRuntime,
    },
    agent_store::{AgentRun, AgentRunStore},
    config::Config,
    tool_binding::{HostToolAuthority, ToolImplementationCatalog},
};
use anyhow::{ensure, Context, Result};
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
    let mut names = vec![root.to_owned()];
    let mut visited = std::collections::HashSet::new();
    let mut definitions = BTreeMap::new();
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
        let agent = &resource.definition.agent;
        definitions.insert(
            agent.model.clone(),
            config
                .models
                .get(&agent.model)
                .context("model alias not found")?
                .clone(),
        );
        names.extend(agent.delegation.allow.iter().cloned());
    }
    Ok(definitions)
}
