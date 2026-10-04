//! Bounded direct execution of static agent resources. Runtime persistence is
//! intentionally writable; model-requested retrieval uses read-only connections.
mod checkpoint;
pub mod cli;
mod control;
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
    agent_store::{
        AccountingOverflow, AgentRun, AgentRunStore, BudgetDecision, FailureReason, LimitReason,
        RunBudgets, RunOutcome, ToolOutcome,
    },
    app_store::SqliteAppStore,
    config::Config,
    tool_binding::{self, HostToolAuthority},
    traits::{ToolContext, ToolRegistry, ToolRuntimeDispatch},
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

fn run_budgets(resource: &LoadedAgentResource) -> RunBudgets {
    let limits = &resource.definition.agent.execution;
    RunBudgets {
        max_turns: Some(u64::from(limits.max_turns)),
        timeout_seconds: Some(limits.timeout_seconds),
        max_total_tokens: limits.max_total_tokens,
        max_tool_calls: limits.max_tool_calls,
    }
}
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
    tool_bindings: Arc<std::collections::BTreeMap<String, tool_binding::LoadedToolResource>>,
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
        ensure!(
            [control::BLOCKED, control::REQUEST_USER_INPUT]
                .iter()
                .all(|name| tools.find(name).is_none()),
            "runtime control tool name is reserved"
        );
        control::register(&mut tools);
        Ok(Self {
            config: Arc::new(config),
            root,
            store,
            tools: Arc::new(tools),
            models: Arc::new(models),
            resources: Arc::new(Default::default()),
            tool_bindings: Arc::new(Default::default()),
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
    pub async fn with_tool_bindings(self, directories: &[ResourceDirectory]) -> Result<Self> {
        let mut authority = HostToolAuthority::new(&self.root, vec![Capability::ReadOnly])?;
        authority.enroll_path(&self.root)?;
        let sources = self
            .config
            .connectors
            .filesystem
            .keys()
            .map(|name| format!("filesystem:{name}"))
            .chain(
                self.config
                    .connectors
                    .git
                    .keys()
                    .map(|name| format!("git:{name}")),
            )
            .chain(
                self.config
                    .connectors
                    .s3
                    .keys()
                    .map(|name| format!("s3:{name}")),
            )
            .chain(
                self.config
                    .connectors
                    .script
                    .keys()
                    .map(|name| format!("script:{name}")),
            );
        for source in sources {
            authority.enroll_source(source)?;
        }
        let catalog = tool_binding::core_catalog()?;
        self.bind_tool_resources(directories, &catalog, Arc::new(authority))
            .await
    }

    /// Bind resources using a catalog and authority supplied by a trusted embedding host.
    pub async fn with_tool_binding_catalog(
        self,
        directories: &[ResourceDirectory],
        catalog: &tool_binding::ToolImplementationCatalog,
        authority: Arc<HostToolAuthority>,
    ) -> Result<Self> {
        self.bind_tool_resources(directories, catalog, authority)
            .await
    }

    async fn bind_tool_resources(
        mut self,
        directories: &[ResourceDirectory],
        catalog: &tool_binding::ToolImplementationCatalog,
        authority: Arc<HostToolAuthority>,
    ) -> Result<Self> {
        let loaded = tool_binding::load_resources(directories, &self.config)?;
        if loaded.is_empty() {
            return Ok(self);
        }
        ensure!(
            loaded.keys().all(|name| !control::is_reserved(name)),
            "runtime control tool name is reserved"
        );
        let local = loaded
            .iter()
            .filter(|(_, resource)| {
                tool_binding::mcp_reference(&resource.definition.tool.implementation).is_none()
            })
            .map(|(name, resource)| (name.clone(), resource.clone()))
            .collect();
        let bindings = tool_binding::bind_resources(&local, catalog, authority).await?;
        self.tool_bindings = Arc::new(loaded);
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
            .create_run_with_budgets(
                &agent.name,
                &resource.version,
                &agent.model,
                input,
                run_budgets(resource),
            )
            .await?;
        let files = match files::acquire(&self.root, &run.id) {
            Ok(files) => files,
            Err(_) => {
                self.store
                    .finish_run(
                        &run.id,
                        RunOutcome::FailedWithReason {
                            code: FailureReason::RecoveryRejected,
                            error: "run ownership is unavailable".into(),
                        },
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
                    .finish_run(
                        &run.id,
                        RunOutcome::FailedWithReason {
                            code: FailureReason::RecoveryRejected,
                            error: error.to_string(),
                        },
                    )
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
                    Ok(Ok(outcome)) => outcome,
                    Ok(Err(error)) => RunOutcome::FailedWithReason {
                        code: if error.downcast_ref::<AccountingOverflow>().is_some() {
                            FailureReason::AccountingError
                        } else {
                            FailureReason::ToolError
                        },
                        error: error.to_string(),
                    },
                    Err(_) => RunOutcome::LimitExceeded { code: LimitReason::Duration },
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
            let tool = self
                .tools
                .find(name)
                .or_else(|| external.find(name))
                .context("unsupported runtime tool declaration")?;
            if tool.runtime_dispatch() == ToolRuntimeDispatch::AgentDelegation {
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
            let dispatch = tool.runtime_dispatch();
            if !matches!(dispatch, ToolRuntimeDispatch::RunControl(_)) {
                let capabilities = tool
                    .capabilities()
                    .context("tool capability metadata is unavailable")?;
                ensure!(
                    self.policy.authorize(&agent.permissions, &capabilities)
                        != Authorization::Denied,
                    "tool capability is not permitted by agent and host policy"
                );
            }
            let mut parameters = tool.parameters_schema();
            if dispatch == ToolRuntimeDispatch::AgentDelegation {
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
    ) -> Result<RunOutcome> {
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
                        "tool_binding_contract": tool_binding::CATALOG_CONTRACT_VERSION,
                        "tool_bindings": self.selected_binding_metadata_with(resource, Some(&external))?,
                        "host_policy": {"allow":self.policy.allow, "require_approval":self.policy.require_approval}
                    }),
                )
                .await?;
            let context = ToolContext::new(self.config.clone());
            if restored.is_none() {
                self.prepare_selected_tools(id, resource, &external, &context)
                    .await?;
            }
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
                let recorded = match self
                    .models
                    .generate_recorded_with_budget(&self.store, id, &agent.model, &request)
                    .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        return Ok(RunOutcome::FailedWithReason {
                            code: if error.downcast_ref::<AccountingOverflow>().is_some() {
                                FailureReason::AccountingError
                            } else {
                                FailureReason::ModelError
                            },
                            error: if error.downcast_ref::<AccountingOverflow>().is_some() {
                                "run accounting overflow".into()
                            } else {
                                "model request failed".into()
                            },
                        });
                    }
                };
                if let Some(code) = recorded.limit {
                    return Ok(RunOutcome::LimitExceeded { code });
                }
                let response = recorded.response;
                match response.finish_reason {
                    FinishReason::Completed => {
                        if response.text.len() <= 64 * 1024 {
                            return Ok(RunOutcome::Completed(response.text));
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
                        return Ok(RunOutcome::Completed(format!(
                            "Output saved to {} ({} bytes; sha256 {})",
                            artifact.relative_path, artifact.size, artifact.sha256
                        )));
                    }
                    FinishReason::Length => return Ok(RunOutcome::FailedWithReason {
                        code: FailureReason::ModelOutputTruncated,
                        error: "model output limit reached".into(),
                    }),
                    FinishReason::Refusal => return Ok(RunOutcome::FailedWithReason {
                        code: FailureReason::ModelRefusal,
                        error: "model refused the request".into(),
                    }),
                    FinishReason::ContentFilter => return Ok(RunOutcome::FailedWithReason {
                        code: FailureReason::ContentFiltered,
                        error: "model response was filtered".into(),
                    }),
                    FinishReason::ToolCalls => {}
                }
                let controls: Vec<_> = response
                    .tool_calls
                    .iter()
                    .filter_map(|call| {
                        self.tools
                            .find(&call.name)
                            .or_else(|| external.find(&call.name))
                            .and_then(|tool| match tool.runtime_dispatch() {
                                ToolRuntimeDispatch::RunControl(kind) => Some((kind, call)),
                                _ => None,
                            })
                    })
                    .collect();
                if !controls.is_empty() {
                    if controls.len() != 1 || response.tool_calls.len() != 1 {
                        return Ok(RunOutcome::FailedWithReason {
                            code: FailureReason::InvalidModelResponse,
                            error: "invalid run control response".into(),
                        });
                    }
                    let (kind, call) = controls[0];
                    return Ok(control::outcome(kind, call.arguments.clone()).unwrap_or(
                        RunOutcome::FailedWithReason {
                            code: FailureReason::InvalidModelResponse,
                            error: "invalid run control response".into(),
                        },
                    ));
                }
                // Do not execute tools if the run cannot consume their results.
                if turn + 1 >= agent.execution.max_turns {
                    return Ok(RunOutcome::LimitExceeded {
                        code: LimitReason::ModelTurns,
                    });
                }
                if let BudgetDecision::Exceeded(code) =
                    self.store.check_model_turn_budget(id).await?
                {
                    return Ok(RunOutcome::LimitExceeded { code });
                }
                if response.tool_calls.len() > MAX_TOOL_CALLS_PER_TURN {
                    return Ok(RunOutcome::LimitExceeded {
                        code: LimitReason::ToolCallsPerTurn,
                    });
                }
                if matches!(
                    self.store
                        .check_tool_call_budget(id, response.tool_calls.len())
                        .await?,
                    BudgetDecision::Exceeded(_)
                ) {
                    return Ok(RunOutcome::LimitExceeded {
                        code: LimitReason::ToolCalls,
                    });
                }
                request.messages.push(response.message());
                for call in response.tool_calls {
                    ensure!(
                        matches!(
                            self.store.check_model_turn_budget(id).await?,
                            BudgetDecision::Allowed
                        ),
                        "shared model turn budget exhausted"
                    );
                    self.store
                        .request_tool(id, &call.id, &call.name, &call.arguments)
                        .await?;
                    let tool = self.tools.find(&call.name).or_else(|| external.find(&call.name)).context("tool unavailable")?;
                    let dispatch = tool.runtime_dispatch();
                    let authorization = tool
                        .capabilities()
                        .map(|caps| self.policy.authorize(&agent.permissions, &caps))
                        .unwrap_or(Authorization::Denied);
                    let valid = match dispatch {
                        ToolRuntimeDispatch::AgentDelegation => {
                            self.validate_delegation(resource, &call.arguments)
                        }
                        ToolRuntimeDispatch::Direct => tool.validate_arguments(&call.arguments),
                        ToolRuntimeDispatch::RunControl(_) => {
                            anyhow::bail!("run control reached ordinary dispatch")
                        }
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
                    let approval_arguments = tool.approval_arguments(&call.arguments)?;
                    self.approve_invocation(id, &call.id, &call.name, &approval_arguments, authorization).await?;
                    self.store.start_tool(id, &call.id).await?;
                    let result = match dispatch {
                        ToolRuntimeDispatch::AgentDelegation => {
                            self.invoke(id, &call.id, resource, call.arguments).await
                        }
                        ToolRuntimeDispatch::Direct => tool.execute(call.arguments, &context).await,
                        ToolRuntimeDispatch::RunControl(_) => unreachable!(),
                    };
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
            Ok(RunOutcome::LimitExceeded {
                code: LimitReason::ModelTurns,
            })
        }
        .await;
        for session in &mut sessions {
            session.close().await;
        }
        result
    }

    async fn prepare_selected_tools(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
        external: &ToolRegistry,
        context: &ToolContext,
    ) -> Result<()> {
        for name in &resource.definition.agent.tools {
            let tool = self
                .tools
                .find(name)
                .or_else(|| external.find(name))
                .context("tool unavailable")?;
            let Some(preparation) = tool.preparation() else {
                continue;
            };
            let call_id = format!("prepare:{name}");
            self.store
                .request_tool(id, &call_id, &preparation.name, &preparation.arguments)
                .await?;
            let authorization = self.policy.authorize(
                &resource.definition.agent.permissions,
                &preparation.capabilities,
            );
            self.approve_invocation(
                id,
                &call_id,
                &preparation.name,
                &preparation.arguments,
                authorization,
            )
            .await?;
            self.store.start_tool(id, &call_id).await?;
            match tool.prepare(context).await {
                Ok(()) => {
                    self.store
                        .finish_tool(
                            id,
                            &call_id,
                            ToolOutcome::Completed(json!({"prepared":true})),
                        )
                        .await?;
                }
                Err(_) => {
                    self.store
                        .finish_tool(
                            id,
                            &call_id,
                            ToolOutcome::Failed("tool preparation failed".into()),
                        )
                        .await?;
                    anyhow::bail!("tool preparation failed");
                }
            }
        }
        Ok(())
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
