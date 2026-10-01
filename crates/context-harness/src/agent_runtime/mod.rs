//! Bounded direct execution of static agent resources. Runtime persistence is
//! intentionally writable; model-requested retrieval uses read-only connections.
mod checkpoint;
pub mod cli;
mod delegation;
mod developer;
mod files;
mod mcp;
mod mcp_client;
pub mod policy;
mod terminal;
mod tools;
use policy::{ApprovalHandler, ApprovalRequest, Authorization, DenyApprovals, RuntimePolicy};

use crate::{
    agent_model::{FinishReason, ModelMessage, ModelRegistry, ModelRequest, ModelTool},
    agent_resource::{Capability, LoadedAgentResource, ResourceDirectory},
    agent_store::{AgentRun, AgentRunStore, RunOutcome, ToolOutcome},
    app_store::SqliteAppStore,
    config::Config,
    tool_binding::{self, HostToolAuthority},
    traits::{ToolContext, ToolRegistry},
};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;
const MAX_TOOL_CALLS_PER_TURN: usize = 32;

/// Workspace binding for the existing cwd-scoped CLI. Registry-based workspace
/// selection is not added here. Canonical roots retain identity across symlinks.
pub fn workspace_id(root: &Path) -> Result<String> {
    let root = root.canonicalize()?;
    Ok(format!(
        "local-{:x}",
        Sha256::digest(root.as_os_str().as_encoded_bytes())
    ))
}

#[derive(Clone)]
pub struct AgentRuntime {
    config: Arc<Config>,
    root: PathBuf,
    store: AgentRunStore,
    tools: Arc<ToolRegistry>,
    models: Arc<ModelRegistry>,
    policy: RuntimePolicy,
    approvals: Arc<dyn ApprovalHandler>,
    resources: Arc<std::collections::BTreeMap<String, LoadedAgentResource>>,
    execution: Option<delegation::ExecutionContext>,
}
impl AgentRuntime {
    /// Initialize runtime history and bind all context reads to this config/DB.
    pub async fn new(mut config: Config, root: &Path, models: ModelRegistry) -> Result<Self> {
        let root = root.canonicalize()?;
        if config.db.path.is_relative() {
            config.db.path = root.join(&config.db.path);
        }
        SqliteAppStore::initialize_config(&config).await?;
        let app = SqliteAppStore::connect(&config).await?;
        let store = app.agent_runs(&workspace_id(&root)?)?;
        let mut tools = tools::registry(&config).await?;
        developer::register(&mut tools, &root)?;
        tools.register(Box::new(delegation::InvokeTool));
        Ok(Self {
            config: Arc::new(config),
            root,
            store,
            tools: Arc::new(tools),
            models: Arc::new(models),
            resources: Arc::new(Default::default()),
            execution: None,
            policy: RuntimePolicy::default(),
            approvals: Arc::new(DenyApprovals),
        })
    }

    /// A trusted host may supply an explicit policy and approval UI. Agent TOML
    /// cannot change this ceiling. By default privileged calls need approval,
    /// and the library denies approvals until a handler is installed.
    pub fn with_policy(
        mut self,
        policy: RuntimePolicy,
        approvals: Arc<dyn ApprovalHandler>,
    ) -> Self {
        self.policy = policy;
        self.approvals = approvals;
        self
    }

    /// Resolve and bind standalone tool resources through the trusted core catalog.
    /// Host authority is the existing workspace read boundary; resources only narrow it.
    pub async fn with_tool_bindings(mut self, directories: &[ResourceDirectory]) -> Result<Self> {
        let loaded = tool_binding::load_resources(directories, &self.config)?;
        if loaded.is_empty() {
            return Ok(self);
        }
        let mut authority = HostToolAuthority::new(&self.root, vec![Capability::ReadOnly])?;
        authority.enroll_path(&self.root)?;
        let catalog = tool_binding::core_catalog()?;
        let bindings = tool_binding::bind_resources(&loaded, &catalog, Arc::new(authority)).await?;
        let tools = Arc::get_mut(&mut self.tools).context("runtime tool registry is shared")?;
        for tool in bindings.tools() {
            ensure!(
                tools.find(tool.name()).is_none(),
                "tool binding '{}' conflicts with an existing runtime tool",
                tool.name()
            );
        }
        for tool in bindings.into_tools() {
            tools.register(tool);
        }
        Ok(self)
    }

    pub fn store(&self) -> &AgentRunStore {
        &self.store
    }

    /// Return persisted terminal state even for expected execution failures, so
    /// callers always receive a run ID. Storage errors remain ordinary errors.
    pub async fn run(
        &self,
        resource: &LoadedAgentResource,
        input: &str,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentRun> {
        ensure!(
            !input.trim().is_empty() && input.len() <= 64 * 1024,
            "input must contain 1–65536 bytes"
        );
        let agent = &resource.definition.agent;
        let run = self
            .store
            .create_run(&agent.name, &resource.version, &agent.model, input)
            .await?;
        let files = match files::acquire(&self.root, &run.id) {
            Ok(files) => files,
            Err(_) => {
                self.store
                    .finish_run(
                        &run.id,
                        RunOutcome::Failed("run ownership is unavailable".into()),
                    )
                    .await?;
                return self
                    .store
                    .get_run(&run.id)
                    .await?
                    .context("run disappeared");
            }
        };
        let mut runtime = self.clone();
        runtime.execution = Some(delegation::ExecutionContext {
            remaining: Arc::new(std::sync::atomic::AtomicU32::new(agent.execution.max_turns)),
            deadline: run.created_at.saturating_add(
                i64::try_from(agent.execution.timeout_seconds)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(1000),
            ),
            ancestry: vec![agent.name.clone()],
        });
        runtime.drive(&run, resource, None, 0, cancel, &files).await
    }

    async fn drive(
        &self,
        run: &AgentRun,
        resource: &LoadedAgentResource,
        request: Option<ModelRequest>,
        start_turn: u32,
        mut cancel: watch::Receiver<bool>,
        files: &files::RunFiles,
    ) -> Result<AgentRun> {
        let remaining = match self.remaining_time(run, resource) {
            Ok(remaining) => remaining,
            Err(error) => {
                self.store
                    .finish_run(&run.id, RunOutcome::Failed(error.to_string()))
                    .await?;
                return self
                    .store
                    .get_run(&run.id)
                    .await?
                    .context("run disappeared");
            }
        };
        let outcome = tokio::select! {
            biased;
            _ = cancelled(&mut cancel) => RunOutcome::Cancelled,
            result = tokio::time::timeout(remaining, self.execute(&run.id, resource, &run.input, request, start_turn, files)) => {
                match result {
                    Ok(Ok(output)) => RunOutcome::Completed(output),
                    Ok(Err(error)) => RunOutcome::Failed(error.to_string()),
                    Err(_) => RunOutcome::Failed("execution timeout".into()),
                }
            }
        };
        self.store
            .finish_run(&run.id, outcome)
            .await
            .with_context(|| format!("could not finalize run {}", run.id))?;
        self.store
            .get_run(&run.id)
            .await?
            .context("run disappeared")
    }

    fn remaining_time(&self, run: &AgentRun, resource: &LoadedAgentResource) -> Result<Duration> {
        let budget = i64::try_from(resource.definition.agent.execution.timeout_seconds)?
            .checked_mul(1000)
            .context("timeout is too large")?;
        let deadline = run
            .created_at
            .checked_add(budget)
            .context("timeout is too large")?;
        let deadline = self
            .execution
            .as_ref()
            .map_or(deadline, |context| deadline.min(context.deadline));
        let remaining = deadline.saturating_sub(chrono::Utc::now().timestamp_millis());
        ensure!(remaining > 0, "original run deadline has expired");
        Ok(Duration::from_millis(remaining as u64))
    }

    fn declarations(&self, resource: &LoadedAgentResource) -> Result<Vec<ModelTool>> {
        self.declarations_with(resource, &ToolRegistry::new())
    }

    fn declarations_with(
        &self,
        resource: &LoadedAgentResource,
        external: &ToolRegistry,
    ) -> Result<Vec<ModelTool>> {
        let agent = &resource.definition.agent;
        let mut declarations = Vec::new();
        for name in &agent.tools {
            if name == "agent.invoke" {
                ensure!(
                    !agent.delegation.allow.is_empty()
                        && agent
                            .delegation
                            .allow
                            .iter()
                            .all(|name| self.resources.contains_key(name)),
                    "delegation targets unavailable"
                );
            }
            let tool = self
                .tools
                .find(name)
                .or_else(|| external.find(name))
                .context("unsupported runtime tool declaration")?;
            let capabilities = tool
                .capabilities()
                .context("tool capability metadata is unavailable")?;
            ensure!(
                self.policy.authorize(&agent.permissions, &capabilities) != Authorization::Denied,
                "tool capability is not permitted by agent and host policy"
            );
            let mut parameters = tool.parameters_schema();
            if name == "agent.invoke" {
                parameters["properties"]["agent"]["enum"] = json!(agent.delegation.allow);
            }
            declarations.push(ModelTool {
                name: name.clone(),
                description: tool.description().into(),
                parameters,
            });
        }
        Ok(declarations)
    }

    async fn execute(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
        input: &str,
        restored: Option<ModelRequest>,
        start_turn: u32,
        files: &files::RunFiles,
    ) -> Result<String> {
        let agent = &resource.definition.agent;
        ensure!(
            agent.execution.max_turns > 0 && agent.execution.timeout_seconds > 0,
            "invalid execution limits"
        );
        let (external, mut sessions) = self.external_tools(id, resource).await?;
        let result = async {
            let declarations = self.declarations_with(resource, &external)?;
            self.store
                .append_event(
                    id,
                    "context.resolved",
                    &json!({
                        "workspace_root": self.root, "agent_version": resource.version,
                        "tools": agent.tools, "retrieval": "keyword",
                        "host_policy": {"allow":self.policy.allow, "require_approval":self.policy.require_approval}
                    }),
                )
                .await?;
            let context = ToolContext::new(self.config.clone());
            let mut request = restored.unwrap_or_else(|| ModelRequest {
                messages: vec![
                    ModelMessage::System {
                        content: resource.definition.prompt.system.clone(),
                    },
                    ModelMessage::User {
                        content: input.into(),
                    },
                ],
                tools: declarations,
                ..Default::default()
            });
            for turn in start_turn..agent.execution.max_turns {
                self.checkpoint(id, resource, &request, turn).await?;
                if let Some(context) = &self.execution { context.consume()?; }
                let response = self
                    .models
                    .generate_recorded(&self.store, id, &agent.model, &request)
                    .await?;
                match response.finish_reason {
                    FinishReason::Completed => {
                        if response.text.len() <= 64 * 1024 {
                            return Ok(response.text);
                        }
                        let artifact = files.write_artifact("output.txt", response.text.as_bytes())?;
                        self.store
                            .record_artifact(
                                id,
                                &artifact.relative_path,
                                &artifact.sha256,
                                artifact.size,
                            )
                            .await?;
                        return Ok(format!(
                            "Output saved to {} ({} bytes; sha256 {})",
                            artifact.relative_path, artifact.size, artifact.sha256
                        ));
                    }
                    FinishReason::Length => anyhow::bail!("model output limit reached"),
                    FinishReason::Refusal => anyhow::bail!("model refused the request"),
                    FinishReason::ContentFilter => anyhow::bail!("model response was filtered"),
                    FinishReason::ToolCalls => {}
                }
                if let Some(context) = &self.execution {
                    ensure!(context.remaining.load(std::sync::atomic::Ordering::SeqCst) > 0, "shared model turn budget exhausted");
                }
                // Do not execute tools if the run cannot consume their results.
                ensure!(
                    turn + 1 < agent.execution.max_turns,
                    "maximum model turns reached"
                );
                ensure!(
                    response.tool_calls.len() <= MAX_TOOL_CALLS_PER_TURN,
                    "too many tool calls in one turn"
                );
                request.messages.push(response.message());
                for call in response.tool_calls {
                    if let Some(context) = &self.execution {
                        ensure!(context.remaining.load(std::sync::atomic::Ordering::SeqCst) > 0, "shared model turn budget exhausted");
                    }
                    self.store
                        .request_tool(id, &call.id, &call.name, &call.arguments)
                        .await?;
                    let tool = self.tools.find(&call.name).or_else(|| external.find(&call.name)).context("tool unavailable")?;
                    let authorization = tool
                        .capabilities()
                        .map(|caps| self.policy.authorize(&agent.permissions, &caps))
                        .unwrap_or(Authorization::Denied);
                    let valid = if matches!(call.name.as_str(), "search" | "get") {
                        tools::validate(&call.name, &call.arguments)
                    } else if call.name == "agent.invoke" {
                        self.validate_delegation(resource, &call.arguments)
                    } else if call.name.starts_with("mcp.") {
                        mcp_client::validate_arguments(&call.arguments)
                    } else if developer::is_tool(&call.name) {
                        developer::validate(&self.root, &call.name, &call.arguments)
                    } else {
                        tool.validate_arguments(&call.arguments)
                    };
                    if !agent.tools.contains(&call.name)
                        || authorization == Authorization::Denied
                        || valid.is_err()
                    {
                        self.store
                            .finish_tool(
                                id,
                                &call.id,
                                ToolOutcome::Denied(
                                    "tool permission or argument validation rejected".into(),
                                ),
                            )
                            .await?;
                        anyhow::bail!("tool permission or argument validation rejected");
                    }
                    self.approve_invocation(id, &call.id, &call.name, &call.arguments, authorization).await?;
                    self.store.start_tool(id, &call.id).await?;
                    let result = if call.name == "agent.invoke" {
                        self.invoke(id, &call.id, resource, call.arguments).await
                    } else {tool.execute(call.arguments, &context).await};
                    match result {
                        Ok(result) => {
                            let content = serde_json::to_string(&result)?;
                            if content.len() > MAX_TOOL_RESULT_BYTES {
                                self.store
                                    .finish_tool(
                                        id,
                                        &call.id,
                                        ToolOutcome::Failed("tool result exceeds 1 MiB".into()),
                                    )
                                    .await?;
                                anyhow::bail!("tool result exceeds 1 MiB");
                            }
                            self.store
                                .finish_tool(id, &call.id, ToolOutcome::Completed(result))
                                .await?;
                            request.messages.push(ModelMessage::Tool {
                                call_id: call.id,
                                content,
                            });
                        }
                        Err(_) => {
                            // Tool errors may contain indexed data. Persist a category,
                            // not arbitrary error strings from tool implementations.
                            self.store
                                .finish_tool(
                                    id,
                                    &call.id,
                                    ToolOutcome::Failed("context tool execution failed".into()),
                                )
                                .await?;
                            anyhow::bail!("context tool execution failed");
                        }
                    }
                }
            }
            anyhow::bail!("maximum model turns reached")
        }
        .await;
        for session in &mut sessions {
            session.close().await;
        }
        result
    }
}

async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}
