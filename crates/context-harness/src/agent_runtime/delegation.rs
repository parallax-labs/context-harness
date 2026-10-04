//! Explicit, sequential delegation with inherited authority and shared budgets.
use super::*;
use crate::{
    agent_resource::{Capability, Permissions},
    tool_binding::{self, ToolTrustClass},
    traits::{Tool, ToolRuntimeDispatch},
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct ExecutionContext {
    pub deadline: i64,
    pub ancestry: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InvokeArgs {
    agent: String,
    input: String,
}
pub(super) fn validate(value: &Value) -> Result<()> {
    let args: InvokeArgs = serde_json::from_value(value.clone())?;
    ensure!(
        !args.agent.is_empty() && !args.input.trim().is_empty() && args.input.len() <= 64 * 1024,
        "invalid delegation arguments"
    );
    Ok(())
}
pub(super) struct InvokeTool;
#[async_trait]
impl Tool for InvokeTool {
    fn name(&self) -> &str {
        "agent.invoke"
    }
    fn description(&self) -> &str {
        "Invoke one allowed agent with inherited permissions and a shared execution budget"
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object","properties":{"agent":{"type":"string"},"input":{"type":"string","minLength":1,"maxLength":65536}},"required":["agent","input"],"additionalProperties":false})
    }
    fn capabilities(&self) -> Option<Vec<Capability>> {
        Some(vec![Capability::AgentDelegate])
    }
    fn binding_metadata(&self) -> Option<Value> {
        Some(tool_binding::compatibility_binding_metadata(
            self.name(),
            "builtin.agent.invoke",
            self.parameters_schema(),
            self.capabilities().unwrap(),
            ToolTrustClass::Builtin,
        ))
    }
    fn runtime_dispatch(&self) -> ToolRuntimeDispatch {
        ToolRuntimeDispatch::AgentDelegation
    }
    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        validate(arguments)
    }
    async fn execute(&self, _: Value, _: &ToolContext) -> Result<Value> {
        anyhow::bail!("delegation requires runtime context")
    }
}
impl RuntimePolicy {
    pub(super) fn inherited(&self, parent: &Permissions) -> Self {
        let mut allow = Vec::new();
        let mut require_approval = Vec::new();
        for capability in [
            Capability::ReadOnly,
            Capability::WorkspaceWrite,
            Capability::ProcessExecute,
            Capability::Network,
            Capability::ExternalSideEffect,
            Capability::AgentDelegate,
        ] {
            match self.authorize(parent, &[capability]) {
                Authorization::Allowed => allow.push(capability),
                Authorization::Approval(_) => require_approval.push(capability),
                Authorization::Denied => {}
            }
        }
        Self {
            allow,
            require_approval,
        }
    }
}
impl AgentRuntime {
    /// Supply an immutable resource catalog from the same resolution context as
    /// the root. Child names never trigger a second filesystem discovery.
    pub fn with_resources(mut self, resources: BTreeMap<String, LoadedAgentResource>) -> Self {
        self.resources = Arc::new(resources);
        self
    }
    pub(super) fn validate_delegation(
        &self,
        parent: &LoadedAgentResource,
        arguments: &Value,
    ) -> Result<()> {
        validate(arguments)?;
        let args: InvokeArgs = serde_json::from_value(arguments.clone())?;
        ensure!(
            parent
                .definition
                .agent
                .delegation
                .allow
                .contains(&args.agent),
            "delegation target is not allowed"
        );
        ensure!(
            self.resources.contains_key(&args.agent),
            "delegation target unavailable"
        );
        let context = self
            .execution
            .as_ref()
            .context("delegation execution context unavailable")?;
        ensure!(
            context.ancestry.len() <= 4,
            "maximum delegation depth reached"
        );
        ensure!(
            !context.ancestry.contains(&args.agent),
            "delegation cycle rejected"
        );
        Ok(())
    }

    pub(super) async fn invoke(
        &self,
        id: &str,
        call_id: &str,
        parent: &LoadedAgentResource,
        arguments: Value,
    ) -> Result<Value> {
        self.validate_delegation(parent, &arguments)?;
        let args: InvokeArgs = serde_json::from_value(arguments)?;
        ensure!(
            parent
                .definition
                .agent
                .delegation
                .allow
                .contains(&args.agent),
            "delegation target is not allowed"
        );
        let resource = self
            .resources
            .get(&args.agent)
            .context("delegation target unavailable")?;
        let context = self
            .execution
            .as_ref()
            .context("delegation execution context unavailable")?;
        ensure!(
            context.ancestry.len() <= 4,
            "maximum delegation depth reached"
        );
        ensure!(
            !context.ancestry.contains(&args.agent),
            "delegation cycle rejected"
        );
        let child = self
            .store
            .create_child_run_with_budgets(
                id,
                call_id,
                &resource.definition.agent.name,
                &resource.version,
                &resource.definition.agent.model,
                &args.input,
                run_budgets(resource),
            )
            .await?;
        let mut runtime = self.clone();
        runtime.policy = self.policy.inherited(&parent.definition.agent.permissions);
        let mut context = context.clone();
        context.ancestry.push(args.agent);
        context.deadline = context.deadline.min(
            child.created_at.saturating_add(
                i64::try_from(resource.definition.agent.execution.timeout_seconds)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(1000),
            ),
        );
        runtime.execution = Some(context);
        let result = async {
            let files = files::acquire(&self.root, &child.id)?;
            let (_sender, cancel) = watch::channel(false);
            Box::pin(runtime.drive(&child, resource, None, 0, cancel, &files)).await
        }
        .await;
        let child = match result {
            Ok(child) => child,
            Err(_) => {
                if self
                    .store
                    .get_run(&child.id)
                    .await?
                    .is_some_and(|run| run.status == "running")
                {
                    self.store
                        .finish_run(
                            &child.id,
                            RunOutcome::Failed("delegated execution failed".into()),
                        )
                        .await?;
                }
                anyhow::bail!("delegated execution failed")
            }
        };
        ensure!(
            child.status == "completed",
            "delegated execution did not complete"
        );
        Ok(
            json!({"run_id":child.id,"agent":child.agent_name,"status":child.status,"output":child.output}),
        )
    }
}
