//! Versioned, bounded snapshots at model boundaries. Recovery never replays a
//! tool from a partially persisted turn, even if it appears to have completed.
use super::*;
use crate::agent_store::{RecoveryDisposition, RunUsage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const LEGACY_VERSION: i64 = 1;
const VERSION: i64 = 2;
const BOUNDED_VERSION: i64 = 3;
const OUTCOME_SCHEMA_VERSION: u32 = 1;
const LIMIT: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotV1 {
    run_id: String,
    workspace: String,
    agent_version: String,
    binding: String,
    next_turn: u32,
    request: ModelRequest,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotV2 {
    run_id: String,
    workspace: String,
    agent_version: String,
    binding: String,
    next_turn: u32,
    request: ModelRequest,
    budgets: RunBudgets,
    usage: RunUsage,
    original_deadline: i64,
    outcome_schema_version: u32,
    projection: context::ProjectionMetadata,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotV3 {
    run_id: String,
    workspace: String,
    agent_version: String,
    binding: String,
    next_turn: u32,
    request: ModelRequest,
    budgets: RunBudgets,
    usage: RunUsage,
    original_deadline: i64,
    outcome_schema_version: u32,
    projection: context::ProjectionMetadata,
    logical_indexes: Vec<u64>,
    source_message_count: u64,
    source_exchange_count: u64,
}

pub(super) struct CheckpointContext<'a> {
    pub request: &'a ModelRequest,
    pub projection: &'a context::ProjectionMetadata,
    pub logical: Option<(&'a [u64], u64, u64)>,
}

struct RestoredSnapshot {
    run_id: String,
    workspace: String,
    agent_version: String,
    binding: String,
    next_turn: u32,
    request: ModelRequest,
    v2: Option<SnapshotV2State>,
    bounded: Option<(Vec<u64>, u64, u64)>,
}

struct SnapshotV2State {
    budgets: RunBudgets,
    usage: RunUsage,
    original_deadline: i64,
    outcome_schema_version: u32,
    projection: context::ProjectionMetadata,
}

fn usage_contains(current: &RunUsage, checkpoint: &RunUsage) -> bool {
    fn contains(current: Option<u64>, checkpoint: Option<u64>) -> bool {
        match (current, checkpoint) {
            (_, None) => true,
            (Some(current), Some(checkpoint)) => current >= checkpoint,
            (None, Some(_)) => false,
        }
    }
    current.model_turns >= checkpoint.model_turns
        && contains(current.input_tokens, checkpoint.input_tokens)
        && contains(current.output_tokens, checkpoint.output_tokens)
        && contains(current.total_tokens, checkpoint.total_tokens)
        && current.responses_with_usage >= checkpoint.responses_with_usage
        && current.responses_without_usage >= checkpoint.responses_without_usage
        && current.tool_calls >= checkpoint.tool_calls
}
impl AgentRuntime {
    fn uses_unrecoverable_tool(&self, resource: &LoadedAgentResource) -> bool {
        resource.definition.agent.tools.iter().any(|name| {
            name.starts_with("mcp.")
                || name == "agent.invoke"
                || self.tool_bindings.get(name).is_some_and(|binding| {
                    tool_binding::mcp_reference(&binding.definition.tool.implementation).is_some()
                })
        })
    }

    pub(super) fn selected_binding_metadata(
        &self,
        resource: &LoadedAgentResource,
    ) -> Result<Value> {
        self.selected_binding_metadata_with(resource, None)
    }

    pub(super) fn selected_binding_metadata_with(
        &self,
        resource: &LoadedAgentResource,
        external: Option<&ToolRegistry>,
    ) -> Result<Value> {
        let mut bindings = serde_json::Map::new();
        for name in &resource.definition.agent.tools {
            if let Some(metadata) = self
                .tools
                .find(name)
                .or_else(|| external.and_then(|tools| tools.find(name)))
                .and_then(|tool| tool.binding_metadata())
            {
                bindings.insert(name.clone(), metadata);
            }
        }
        Ok(Value::Object(bindings))
    }

    fn binding(&self, resource: &LoadedAgentResource, schema_version: i64) -> Result<String> {
        let alias = &resource.definition.agent.model;
        let (provider, model) = self.models.identity(alias)?;
        let binding = if schema_version == LEGACY_VERSION {
            json!({
                "runtime_schema": LEGACY_VERSION,
                "db":self.config.db.path.canonicalize()?,
                "candidate_k_keyword":self.config.retrieval.candidate_k_keyword.clamp(1,1000),
                "provider":provider,"model":model,"definition":self.config.models.get(alias),
                "tools":self.declarations(resource)?,
                "tool_binding_contract": tool_binding::CATALOG_CONTRACT_VERSION,
                "tool_bindings": self.selected_binding_metadata(resource)?,
                "policy_allow":self.policy.allow,"policy_approval":self.policy.require_approval,
            })
        } else {
            let implementation = self.models.implementation_identity(alias)?;
            json!({
                "runtime_schema": schema_version,
                "db":self.config.db.path.canonicalize()?,
                "candidate_k_keyword":self.config.retrieval.candidate_k_keyword.clamp(1,1000),
                "provider":provider,"model":model,"definition":self.config.models.get(alias),
                "provider_implementation":{"id":implementation.id(),"version":implementation.version()},
                "tools":self.declarations(resource)?,
                "tool_binding_contract": tool_binding::CATALOG_CONTRACT_VERSION,
                "tool_bindings": self.selected_binding_metadata(resource)?,
                "policy_allow":self.policy.allow,"policy_approval":self.policy.require_approval,
            })
        };
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&binding)?)
        ))
    }
    pub(super) async fn checkpoint(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
        checkpoint_context: CheckpointContext<'_>,
        turn: u32,
        baseline: Option<&AgentRun>,
    ) -> Result<()> {
        // External sessions and delegated budgets cannot be recovered independently.
        if self
            .execution
            .as_ref()
            .is_some_and(|context| context.ancestry.len() > 1)
        {
            return Ok(());
        }
        if self.uses_unrecoverable_tool(resource) {
            return Ok(());
        }
        checkpoint_context.request.validate()?;
        let current;
        let run = if let Some(run) = baseline {
            run
        } else {
            current = self
                .store
                .get_run_raw(id)
                .await?
                .context("run disappeared")?;
            &current
        };
        let timeout = run
            .budgets
            .timeout_seconds
            .context("run timeout budget is unavailable")?;
        let original_deadline = run
            .created_at
            .checked_add(
                i64::try_from(timeout)?
                    .checked_mul(1000)
                    .context("timeout is too large")?,
            )
            .context("timeout is too large")?;
        let common = SnapshotV2 {
            run_id: id.into(),
            workspace: workspace_id(&self.root)?,
            agent_version: resource.version.clone(),
            binding: self.binding(
                resource,
                if checkpoint_context.logical.is_some() {
                    BOUNDED_VERSION
                } else {
                    VERSION
                },
            )?,
            next_turn: turn,
            request: checkpoint_context.request.clone(),
            budgets: run.budgets.0.clone(),
            usage: run.usage.0.clone(),
            original_deadline,
            outcome_schema_version: OUTCOME_SCHEMA_VERSION,
            projection: checkpoint_context.projection.clone(),
        };
        let (schema_version, value) =
            if let Some((logical_indexes, source_message_count, source_exchange_count)) =
                checkpoint_context.logical
            {
                let snapshot = SnapshotV3 {
                    run_id: common.run_id,
                    workspace: common.workspace,
                    agent_version: common.agent_version,
                    binding: common.binding,
                    next_turn: common.next_turn,
                    request: common.request,
                    budgets: common.budgets,
                    usage: common.usage,
                    original_deadline: common.original_deadline,
                    outcome_schema_version: common.outcome_schema_version,
                    projection: common.projection,
                    logical_indexes: logical_indexes.to_vec(),
                    source_message_count,
                    source_exchange_count,
                };
                (BOUNDED_VERSION, serde_json::to_value(snapshot)?)
            } else {
                (VERSION, serde_json::to_value(common)?)
            };
        ensure!(
            serde_json::to_vec(&value)?.len() <= LIMIT,
            "checkpoint exceeds 16 MiB"
        );
        self.store
            .save_checkpoint(id, schema_version, i64::from(turn), &value)
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
            !self.uses_unrecoverable_tool(resource),
            "MCP-backed or delegating runs cannot resume; tree/session recovery is unsupported"
        );
        ensure!(
            self.store.lineage(id).await?.parent_run_id.is_none(),
            "delegated child runs cannot resume independently"
        );
        let files = files::acquire(&self.root, id)?;
        let run = self.store.get_run(id).await?.context("run disappeared")?;
        match self.store.recovery_disposition(id).await? {
            RecoveryDisposition::ResumeEligible => {}
            RecoveryDisposition::ReconciliationRequired => {
                anyhow::bail!("run requires reconciliation before resume")
            }
            RecoveryDisposition::RestartRequired => anyhow::bail!(
                "run requires restart because its checkpoint, deadline, delegation, or budget is unavailable"
            ),
            RecoveryDisposition::Complete => anyhow::bail!("completed run cannot resume"),
            RecoveryDisposition::Suspended => {
                anyhow::bail!("suspended run requires an explicit continuation")
            }
        }
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
            matches!(
                checkpoint.schema_version,
                LEGACY_VERSION | VERSION | BOUNDED_VERSION
            ),
            "unsupported checkpoint version"
        );
        ensure!(
            serde_json::to_vec(&checkpoint.state.0)?.len() <= LIMIT,
            "checkpoint exceeds 16 MiB"
        );
        let state = match checkpoint.schema_version {
            LEGACY_VERSION => {
                let legacy: SnapshotV1 = serde_json::from_value(checkpoint.state.0)?;
                RestoredSnapshot {
                    run_id: legacy.run_id,
                    workspace: legacy.workspace,
                    agent_version: legacy.agent_version,
                    binding: legacy.binding,
                    next_turn: legacy.next_turn,
                    request: legacy.request,
                    v2: None,
                    bounded: None,
                }
            }
            VERSION => {
                let current: SnapshotV2 = serde_json::from_value(checkpoint.state.0)?;
                RestoredSnapshot {
                    run_id: current.run_id,
                    workspace: current.workspace,
                    agent_version: current.agent_version,
                    binding: current.binding,
                    next_turn: current.next_turn,
                    request: current.request,
                    v2: Some(SnapshotV2State {
                        budgets: current.budgets,
                        usage: current.usage,
                        original_deadline: current.original_deadline,
                        outcome_schema_version: current.outcome_schema_version,
                        projection: current.projection,
                    }),
                    bounded: None,
                }
            }
            BOUNDED_VERSION => {
                let current: SnapshotV3 = serde_json::from_value(checkpoint.state.0)?;
                RestoredSnapshot {
                    run_id: current.run_id,
                    workspace: current.workspace,
                    agent_version: current.agent_version,
                    binding: current.binding,
                    next_turn: current.next_turn,
                    request: current.request,
                    bounded: Some((
                        current.logical_indexes,
                        current.source_message_count,
                        current.source_exchange_count,
                    )),
                    v2: Some(SnapshotV2State {
                        budgets: current.budgets,
                        usage: current.usage,
                        original_deadline: current.original_deadline,
                        outcome_schema_version: current.outcome_schema_version,
                        projection: current.projection,
                    }),
                }
            }
            _ => unreachable!(),
        };
        ensure!(
            state.run_id == id
                && state.workspace == run.workspace_id
                && state.agent_version == run.agent_version
                && state.binding == self.binding(resource, checkpoint.schema_version)?
                && i64::from(state.next_turn) == checkpoint.turn,
            "checkpoint binding changed or is invalid"
        );
        if let Some(v2) = &state.v2 {
            let timeout = run
                .budgets
                .timeout_seconds
                .context("run timeout budget is unavailable")?;
            let deadline = run
                .created_at
                .checked_add(
                    i64::try_from(timeout)?
                        .checked_mul(1000)
                        .context("timeout is too large")?,
                )
                .context("timeout is too large")?;
            let common_valid = v2.outcome_schema_version == OUTCOME_SCHEMA_VERSION
                && v2.budgets == run.budgets.0
                && v2.original_deadline == deadline
                && usage_contains(&run.usage, &v2.usage);
            let projection_valid = if checkpoint.schema_version == BOUNDED_VERSION {
                state
                    .bounded
                    .as_ref()
                    .is_some_and(|(indexes, count, exchanges)| {
                        v2.projection.builder == "bounded_v1"
                            && v2.projection.strategy == "bounded_suffix_v1"
                            && indexes.len() + 1 == state.request.messages.len()
                            && *count >= indexes.len() as u64
                            && *exchanges >= v2.projection.included_exchange_count.unwrap_or(0)
                    })
            } else {
                v2.projection.builder == "compatibility_v1"
                    && v2.projection.strategy == "complete_transcript"
                    && v2.projection.message_count == Some(state.request.messages.len())
                    && v2.projection.tool_count == Some(state.request.tools.len())
            };
            ensure!(
                common_valid && projection_valid,
                "checkpoint run state changed or is invalid"
            );
        }
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
            if checkpoint.schema_version == BOUNDED_VERSION {
                state.request.messages.len() >= 3
                    && serde_json::to_value([
                        &state.request.messages[0],
                        &state.request.messages[2],
                    ])? == serde_json::to_value(initial)?
            } else {
                state.request.messages.len() >= 2
                    && serde_json::to_value(&state.request.messages[..2])?
                        == serde_json::to_value(initial)?
            },
            "checkpoint input changed"
        );
        let invocations = self.store.tool_invocations(id).await?;
        let omitted_exchanges = state
            .v2
            .as_ref()
            .and_then(|snapshot| snapshot.projection.omitted_exchange_count)
            .unwrap_or(0);
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
        let captured_invocations = invocations
            .iter()
            .filter(|invocation| !invocation.tool_name.starts_with("runtime."))
            .count();
        ensure!(
            if checkpoint.schema_version == BOUNDED_VERSION {
                captured_invocations >= restored_results.len()
            } else {
                captured_invocations == restored_results.len()
            },
            "tool history is not captured by checkpoint; manual reconciliation required"
        );
        for invocation in &invocations {
            if invocation.tool_name.starts_with("runtime.") {
                continue;
            }
            ensure!(
                invocation.status == "completed"
                    && invocation.requested_sequence < checkpoint.sequence,
                "tool history is unsafe for recovery; manual reconciliation required"
            );
            if checkpoint.schema_version == BOUNDED_VERSION
                && !restored_results.contains_key(invocation.call_id.as_str())
            {
                ensure!(
                    omitted_exchanges > 0,
                    "tool history omission is not represented by the projection trace"
                );
                continue;
            }
            let call = restored_calls
                .get(invocation.call_id.as_str())
                .context("tool call missing from checkpoint; manual reconciliation required")?;
            ensure!(
                call.name == invocation.tool_name && call.arguments == invocation.arguments.0,
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
                    ensure!(matches!(event.event_type.as_str(),"model.requested"|"model.responded"|"model.failed"|"run.failed"|"run.cancelled"|"run.resumed"|"context.resolved"|"context.projected"),
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
            Some(RestoredRequest {
                request: state.request,
                logical_indexes: state.bounded.as_ref().map(|value| value.0.clone()),
                source_message_count: state.bounded.as_ref().map(|value| value.1),
                source_exchange_count: state.bounded.as_ref().map(|value| value.2),
            }),
            attempts,
            cancel,
            &files,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_v1_snapshot_remains_decodable() {
        let value = json!({
            "run_id":"run",
            "workspace":"workspace",
            "agent_version":"agent-v1",
            "binding":"binding",
            "next_turn":1,
            "request":{
                "messages":[{"role":"user","content":"objective"}],
                "tools":[],
                "output_schema":null,
                "max_output_tokens":null
            }
        });
        let snapshot: SnapshotV1 = serde_json::from_value(value).unwrap();
        assert_eq!(snapshot.run_id, "run");
        assert_eq!(snapshot.next_turn, 1);
        snapshot.request.validate().unwrap();
    }
}
