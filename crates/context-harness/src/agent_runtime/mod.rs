//! Bounded direct execution of static agent resources. Runtime persistence is
//! intentionally writable; model-requested retrieval uses read-only connections.
pub mod cli;
mod developer;
pub mod policy;
mod terminal;
mod tools;
use policy::{ApprovalHandler, ApprovalRequest, Authorization, DenyApprovals, RuntimePolicy};

use crate::{
    agent_model::{FinishReason, ModelMessage, ModelRegistry, ModelRequest, ModelTool},
    agent_resource::LoadedAgentResource,
    agent_store::{AgentRun, AgentRunStore, RunOutcome, ToolOutcome},
    app_store::SqliteAppStore,
    config::Config,
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

pub struct AgentRuntime {
    config: Arc<Config>,
    root: PathBuf,
    store: AgentRunStore,
    tools: ToolRegistry,
    models: ModelRegistry,
    policy: RuntimePolicy,
    approvals: Arc<dyn ApprovalHandler>,
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
        Ok(Self {
            config: Arc::new(config),
            root,
            store,
            tools,
            models,
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

    pub fn store(&self) -> &AgentRunStore {
        &self.store
    }

    /// Return persisted terminal state even for expected execution failures, so
    /// callers always receive a run ID. Storage errors remain ordinary errors.
    pub async fn run(
        &self,
        resource: &LoadedAgentResource,
        input: &str,
        mut cancel: watch::Receiver<bool>,
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
        let outcome = tokio::select! {
            biased;
            _ = cancelled(&mut cancel) => RunOutcome::Cancelled,
            result = tokio::time::timeout(Duration::from_secs(agent.execution.timeout_seconds), self.execute(&run.id, resource, input)) => {
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

    async fn execute(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
        input: &str,
    ) -> Result<String> {
        let agent = &resource.definition.agent;
        ensure!(
            agent.execution.max_turns > 0 && agent.execution.timeout_seconds > 0,
            "invalid execution limits"
        );
        let mut declarations = Vec::new();
        for name in &agent.tools {
            let tool = self
                .tools
                .find(name)
                .context("unsupported runtime tool declaration")?;
            let capabilities = tool
                .capabilities()
                .context("tool capability metadata is unavailable")?;
            ensure!(
                self.policy.authorize(&agent.permissions, &capabilities) != Authorization::Denied,
                "tool capability is not permitted by agent and host policy"
            );
            declarations.push(ModelTool {
                name: name.clone(),
                description: tool.description().into(),
                parameters: tool.parameters_schema(),
            });
        }
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
        let mut request = ModelRequest {
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
        };
        for turn in 0..agent.execution.max_turns {
            let response = self
                .models
                .generate_recorded(&self.store, id, &agent.model, &request)
                .await?;
            match response.finish_reason {
                FinishReason::Completed => return Ok(response.text),
                FinishReason::Length => anyhow::bail!("model output limit reached"),
                FinishReason::Refusal => anyhow::bail!("model refused the request"),
                FinishReason::ContentFilter => anyhow::bail!("model response was filtered"),
                FinishReason::ToolCalls => {}
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
                self.store
                    .request_tool(id, &call.id, &call.name, &call.arguments)
                    .await?;
                let tool = self.tools.find(&call.name).context("tool unavailable")?;
                let authorization = tool
                    .capabilities()
                    .map(|caps| self.policy.authorize(&agent.permissions, &caps))
                    .unwrap_or(Authorization::Denied);
                let valid = if matches!(call.name.as_str(), "search" | "get") {
                    tools::validate(&call.name, &call.arguments)
                } else {
                    developer::validate(&self.root, &call.name, &call.arguments)
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
                if let Authorization::Approval(capabilities) = authorization {
                    self.store
                        .request_approval(id, &call.id, &capabilities)
                        .await?;
                    let granted = self
                        .approvals
                        .approve(&ApprovalRequest {
                            run_id: id.into(),
                            call_id: call.id.clone(),
                            tool: call.name.clone(),
                            capabilities,
                            arguments: call.arguments.clone(),
                        })
                        .await;
                    self.store.decide_approval(id, &call.id, granted).await?;
                    if !granted {
                        self.store
                            .finish_tool(
                                id,
                                &call.id,
                                ToolOutcome::Denied("approval denied".into()),
                            )
                            .await?;
                        anyhow::bail!("tool approval denied");
                    }
                }
                self.store.start_tool(id, &call.id).await?;
                let result = tool.execute(call.arguments, &context).await;
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
