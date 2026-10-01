//! Versioned, bounded snapshots at model boundaries. Recovery never replays a
//! tool from a partially persisted turn, even if it appears to have completed.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const VERSION: i64 = 1;
const LIMIT: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    run_id: String,
    workspace: String,
    agent_version: String,
    binding: String,
    next_turn: u32,
    request: ModelRequest,
}
impl AgentRuntime {
    pub(super) fn selected_binding_metadata(
        &self,
        resource: &LoadedAgentResource,
    ) -> Result<Value> {
        let mut bindings = serde_json::Map::new();
        for name in &resource.definition.agent.tools {
            if let Some(metadata) = self
                .tools
                .find(name)
                .and_then(|tool| tool.binding_metadata())
            {
                bindings.insert(name.clone(), metadata);
            }
        }
        Ok(Value::Object(bindings))
    }

    fn binding(&self, resource: &LoadedAgentResource) -> Result<String> {
        let alias = &resource.definition.agent.model;
        let (provider, model) = self.models.identity(alias)?;
        let binding = json!({
            "runtime_schema": VERSION,
            "db":self.config.db.path.canonicalize()?,
            "candidate_k_keyword":self.config.retrieval.candidate_k_keyword.clamp(1,1000),
            "provider":provider,"model":model,"definition":self.config.models.get(alias),
            "tools":self.declarations(resource)?,
            "tool_binding_contract": tool_binding::CATALOG_CONTRACT_VERSION,
            "tool_bindings": self.selected_binding_metadata(resource)?,
            "policy_allow":self.policy.allow,"policy_approval":self.policy.require_approval,
        });
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&binding)?)
        ))
    }
    pub(super) async fn checkpoint(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
        request: &ModelRequest,
        turn: u32,
    ) -> Result<()> {
        // External sessions and delegated budgets cannot be recovered independently.
        if self
            .execution
            .as_ref()
            .is_some_and(|context| context.ancestry.len() > 1)
        {
            return Ok(());
        }
        if resource
            .definition
            .agent
            .tools
            .iter()
            .any(|name| name.starts_with("mcp.") || name == "agent.invoke")
        {
            return Ok(());
        }
        request.validate()?;
        let snapshot = Snapshot {
            run_id: id.into(),
            workspace: workspace_id(&self.root)?,
            agent_version: resource.version.clone(),
            binding: self.binding(resource)?,
            next_turn: turn,
            request: request.clone(),
        };
        let value = serde_json::to_value(snapshot)?;
        ensure!(
            serde_json::to_vec(&value)?.len() <= LIMIT,
            "checkpoint exceeds 16 MiB"
        );
        self.store
            .save_checkpoint(id, VERSION, i64::from(turn), &value)
            .await?;
        Ok(())
    }

    /// Resume only at a completed conversation boundary. The original resource,
    /// host policy, model binding, turn budget and wall-clock deadline still apply.
    pub async fn resume(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentRun> {
        // Resolve scope before touching filesystem; lock before reading recovery state.
        self.store
            .get_run(id)
            .await?
            .context("run not found in workspace")?;
        ensure!(
            !resource
                .definition
                .agent
                .tools
                .iter()
                .any(|name| name.starts_with("mcp.") || name == "agent.invoke"),
            "MCP-backed or delegating runs cannot resume; tree/session recovery is unsupported"
        );
        ensure!(
            self.store.lineage(id).await?.parent_run_id.is_none(),
            "delegated child runs cannot resume independently"
        );
        let files = files::acquire(&self.root, id)?;
        let run = self.store.get_run(id).await?.context("run disappeared")?;
        ensure!(run.status != "completed", "completed runs cannot resume");
        ensure!(
            run.agent_name == resource.definition.agent.name
                && run.agent_version == resource.version
                && resource.definition.version()? == resource.version
                && run.model == resource.definition.agent.model,
            "agent resource or model alias changed"
        );
        self.remaining_time(&run, resource)?;
        let checkpoint = self
            .store
            .latest_checkpoint(id)
            .await?
            .context("run has no recovery checkpoint")?;
        ensure!(
            checkpoint.schema_version == VERSION,
            "unsupported checkpoint version"
        );
        ensure!(
            serde_json::to_vec(&checkpoint.state.0)?.len() <= LIMIT,
            "checkpoint exceeds 16 MiB"
        );
        let state: Snapshot = serde_json::from_value(checkpoint.state.0)?;
        ensure!(
            state.run_id == id
                && state.workspace == run.workspace_id
                && state.agent_version == run.agent_version
                && state.binding == self.binding(resource)?
                && i64::from(state.next_turn) == checkpoint.turn,
            "checkpoint binding changed or is invalid"
        );
        state.request.validate()?;
        ensure!(
            serde_json::to_value(&state.request.tools)?
                == serde_json::to_value(self.declarations(resource)?)?,
            "checkpoint tool definitions changed"
        );
        let initial = vec![
            ModelMessage::System {
                content: resource.definition.prompt.system.clone(),
            },
            ModelMessage::User {
                content: run.input.clone(),
            },
        ];
        ensure!(
            state.request.messages.len() >= 2
                && serde_json::to_value(&state.request.messages[..2])?
                    == serde_json::to_value(initial)?,
            "checkpoint input changed"
        );
        let invocations = self.store.tool_invocations(id).await?;
        let mut restored_results = std::collections::HashMap::new();
        let mut restored_calls = std::collections::HashMap::new();
        for message in &state.request.messages {
            match message {
                ModelMessage::Tool { call_id, content } => {
                    restored_results.insert(call_id.as_str(), content.as_str());
                }
                ModelMessage::Assistant { tool_calls, .. } => {
                    for call in tool_calls {
                        restored_calls.insert(call.id.as_str(), call);
                    }
                }
                _ => {}
            }
        }
        ensure!(
            invocations
                .iter()
                .filter(|invocation| !invocation.tool_name.starts_with("runtime."))
                .count()
                == restored_results.len(),
            "tool history is not captured by checkpoint; manual reconciliation required"
        );
        for invocation in &invocations {
            if invocation.tool_name.starts_with("runtime.") {
                continue;
            }
            let call = restored_calls
                .get(invocation.call_id.as_str())
                .context("tool call missing from checkpoint; manual reconciliation required")?;
            ensure!(
                invocation.status == "completed"
                    && invocation.requested_sequence < checkpoint.sequence
                    && call.name == invocation.tool_name
                    && call.arguments == invocation.arguments.0,
                "tool history does not match checkpoint; manual reconciliation required"
            );
            let result = invocation
                .result
                .as_ref()
                .context("completed tool result missing")?;
            let serialized = serde_json::to_string(&result.0)?;
            ensure!(
                restored_results.get(invocation.call_id.as_str()) == Some(&serialized.as_str()),
                "checkpoint tool result changed"
            );
        }
        // Account every attempted model call, including an interrupted call. Never
        // reset the turn budget by returning to an earlier checkpoint.
        let mut cursor = 0;
        let mut attempts = 0u32;
        loop {
            let events = self.store.events(id, cursor, 1000).await?;
            if events.is_empty() {
                break;
            }
            for event in events {
                cursor = event.sequence;
                if event.event_type == "model.requested" {
                    attempts = attempts.checked_add(1).context("too many model attempts")?;
                }
                if event.sequence > checkpoint.sequence {
                    ensure!(matches!(event.event_type.as_str(),"model.requested"|"model.responded"|"model.failed"|"run.failed"|"run.cancelled"|"run.resumed"|"context.resolved"),
                        "run has activity beyond its safe checkpoint; manual reconciliation required");
                }
            }
        }
        ensure!(
            attempts >= state.next_turn && attempts < resource.definition.agent.execution.max_turns,
            "model turn budget exhausted or invalid"
        );
        let resumed = self.store.reopen_run(id, run.last_sequence).await?;
        self.drive(
            &resumed,
            resource,
            Some(state.request),
            attempts,
            cancel,
            &files,
        )
        .await
    }
}
