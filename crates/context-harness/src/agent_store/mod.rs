//! Durable runtime history in the application's existing SQLite database.
//!
//! Every handle is bound to one workspace. State changes and their events commit
//! together. Event sequences are allocated by a write statement, avoiding a
//! read/modify/write race between concurrent writers. Timestamps are Unix ms.

use crate::agent_resource::Capability;
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

/// Materialized state of an execution. Inputs and outputs are plain text.
#[derive(Debug, Serialize, FromRow)]
pub struct AgentRun {
    pub id: String,
    pub workspace_id: String,
    pub agent_name: String,
    pub agent_version: String,
    pub model: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub input: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub last_sequence: i64,
}

/// One immutable event; sequence numbers are local to a run and start at one.
#[derive(Debug, Serialize, FromRow)]
pub struct AgentEvent {
    pub run_id: String,
    pub sequence: i64,
    pub timestamp: i64,
    pub event_type: String,
    pub payload: sqlx::types::Json<Value>,
}

/// Versioned execution snapshot. Runtime code owns the state schema; it must
/// include resolved resource/model, conversation, workspace and policy context.
#[derive(Debug, Serialize, FromRow)]
pub struct AgentCheckpoint {
    pub run_id: String,
    pub sequence: i64,
    pub schema_version: i64,
    pub turn: i64,
    pub created_at: i64,
    pub state: sqlx::types::Json<Value>,
}

/// A terminal transition; callers cannot create arbitrary status strings.
pub enum RunOutcome {
    Completed(String),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ToolInvocation {
    pub run_id: String,
    pub call_id: String,
    pub requested_sequence: i64,
    pub tool_name: String,
    pub arguments: sqlx::types::Json<Value>,
    pub status: String,
    pub result: Option<sqlx::types::Json<Value>>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

pub enum ToolOutcome {
    Completed(Value),
    Failed(String),
    Denied(String),
}

/// Workspace-scoped persistence facade, sharing the application connection pool.
#[derive(Clone)]
pub struct AgentRunStore {
    pool: SqlitePool,
    workspace_id: String,
}

impl AgentRunStore {
    pub fn new(pool: SqlitePool, workspace_id: &str) -> Result<Self> {
        ensure!(!workspace_id.trim().is_empty(), "workspace ID is required");
        Ok(Self {
            pool,
            workspace_id: workspace_id.to_owned(),
        })
    }

    /// Create the materialized run and its initial event atomically.
    pub async fn create_run(
        &self,
        agent_name: &str,
        agent_version: &str,
        model: &str,
        input: &str,
    ) -> Result<AgentRun> {
        ensure!(!agent_name.trim().is_empty(), "agent name is required");
        ensure!(
            !agent_version.trim().is_empty(),
            "agent version is required"
        );
        ensure!(!model.trim().is_empty(), "model is required");
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO agent_runs (id, workspace_id, agent_name, agent_version, model, status, created_at, updated_at, input) VALUES (?, ?, ?, ?, ?, 'running', ?, ?, ?)")
            .bind(&id).bind(&self.workspace_id).bind(agent_name).bind(agent_version).bind(model).bind(now).bind(now).bind(input)
            .execute(&mut *tx).await?;
        self.append_in(
            &mut tx,
            &id,
            "run.started",
            &json!({"agent": agent_name, "model": model, "workspace_id": self.workspace_id}),
        )
        .await?;
        tx.commit().await?;
        self.get_run(&id).await?.context("created run missing")
    }

    pub async fn get_run(&self, id: &str) -> Result<Option<AgentRun>> {
        Ok(
            sqlx::query_as("SELECT * FROM agent_runs WHERE id = ? AND workspace_id = ?")
                .bind(id)
                .bind(&self.workspace_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Most recent runs first, with a bounded result count.
    pub async fn history(&self, limit: u32) -> Result<Vec<AgentRun>> {
        Ok(sqlx::query_as(
            "SELECT * FROM agent_runs WHERE workspace_id = ? ORDER BY created_at DESC, id LIMIT ?",
        )
        .bind(&self.workspace_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Append a runtime event. Run, tool lifecycle and checkpoint events use the
    /// dedicated operations so the event log and materialized state agree.
    pub async fn append_event(&self, id: &str, event_type: &str, payload: &Value) -> Result<i64> {
        ensure!(!event_type.trim().is_empty(), "event type is required");
        ensure!(
            !event_type.starts_with("run.")
                && !event_type.starts_with("checkpoint.")
                && !event_type.starts_with("tool.")
                && !event_type.starts_with("approval."),
            "reserved event type"
        );
        let mut tx = self.pool.begin().await?;
        let sequence = self.append_in(&mut tx, id, event_type, payload).await?;
        tx.commit().await?;
        Ok(sequence)
    }

    /// Incremental ordered event reads. Unknown/cross-workspace IDs fail closed.
    pub async fn events(
        &self,
        id: &str,
        after_sequence: i64,
        limit: u32,
    ) -> Result<Vec<AgentEvent>> {
        self.get_run(id)
            .await?
            .context("run not found in workspace")?;
        Ok(sqlx::query_as("SELECT e.* FROM agent_events e JOIN agent_runs r ON r.id = e.run_id WHERE e.run_id = ? AND r.workspace_id = ? AND e.sequence > ? ORDER BY e.sequence LIMIT ?")
            .bind(id).bind(&self.workspace_id).bind(after_sequence).bind(limit).fetch_all(&self.pool).await?)
    }

    /// Persist a checkpoint and checkpoint.created event in one transaction.
    pub async fn save_checkpoint(
        &self,
        id: &str,
        schema_version: i64,
        turn: i64,
        state: &Value,
    ) -> Result<i64> {
        ensure!(
            schema_version > 0 && turn >= 0,
            "invalid checkpoint version or turn"
        );
        let mut tx = self.pool.begin().await?;
        let sequence = self
            .append_in(
                &mut tx,
                id,
                "checkpoint.created",
                &json!({"schema_version": schema_version, "turn": turn}),
            )
            .await?;
        sqlx::query("INSERT INTO agent_checkpoints (run_id, sequence, schema_version, turn, created_at, state) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id).bind(sequence).bind(schema_version).bind(turn).bind(Utc::now().timestamp_millis()).bind(sqlx::types::Json(state))
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(sequence)
    }

    pub async fn latest_checkpoint(&self, id: &str) -> Result<Option<AgentCheckpoint>> {
        self.get_run(id)
            .await?
            .context("run not found in workspace")?;
        Ok(sqlx::query_as("SELECT c.* FROM agent_checkpoints c JOIN agent_runs r ON r.id = c.run_id WHERE c.run_id = ? AND r.workspace_id = ? ORDER BY c.sequence DESC LIMIT 1")
            .bind(id).bind(&self.workspace_id).fetch_optional(&self.pool).await?)
    }

    /// Terminal runs are immutable through this API, including their event log.
    pub async fn finish_run(&self, id: &str, outcome: RunOutcome) -> Result<()> {
        let (status, output, error) = match outcome {
            RunOutcome::Completed(output) => ("completed", Some(output), None),
            RunOutcome::Failed(error) => ("failed", None, Some(error)),
            RunOutcome::Cancelled => ("cancelled", None, None),
        };
        let mut tx = self.pool.begin().await?;
        // Acquire the write lock before reading invocation state. Terminal state
        // and cleanup of interrupted calls commit in the same transaction.
        let found: Option<String> = sqlx::query_scalar("UPDATE agent_runs SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND status = 'running' RETURNING id")
            .bind(id).bind(&self.workspace_id).fetch_optional(&mut *tx).await?;
        ensure!(
            found.is_some(),
            "run not found in workspace or already terminal"
        );
        let unfinished: Vec<String> = sqlx::query_scalar("SELECT call_id FROM tool_invocations WHERE run_id = ? AND status IN ('requested', 'started') ORDER BY requested_sequence")
            .bind(id).fetch_all(&mut *tx).await?;
        ensure!(
            status != "completed" || unfinished.is_empty(),
            "cannot complete run with unfinished tools"
        );
        for call_id in unfinished {
            self.deny_pending_approval(&mut tx, id, &call_id, "run interrupted")
                .await?;
            self.append_in(
                &mut tx,
                id,
                "tool.failed",
                &json!({"call_id": call_id, "error": "run interrupted"}),
            )
            .await?;
            sqlx::query("UPDATE tool_invocations SET status = 'failed', error = 'run interrupted', completed_at = ? WHERE run_id = ? AND call_id = ?")
                .bind(Utc::now().timestamp_millis()).bind(id).bind(call_id).execute(&mut *tx).await?;
        }
        self.append_in(
            &mut tx,
            id,
            &format!("run.{status}"),
            &json!({"output": output, "error": error}),
        )
        .await?;
        sqlx::query("UPDATE agent_runs SET status = ?, output = ?, error = ?, completed_at = updated_at WHERE id = ? AND workspace_id = ?")
            .bind(status).bind(output).bind(error).bind(id).bind(&self.workspace_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn request_tool(
        &self,
        id: &str,
        call_id: &str,
        name: &str,
        arguments: &Value,
    ) -> Result<()> {
        ensure!(
            !call_id.is_empty() && !name.is_empty() && arguments.is_object(),
            "invalid tool invocation"
        );
        let mut tx = self.pool.begin().await?;
        let sequence = self
            .append_in(
                &mut tx,
                id,
                "tool.requested",
                &json!({"call_id": call_id, "tool": name}),
            )
            .await?;
        sqlx::query("INSERT INTO tool_invocations (run_id, call_id, requested_sequence, tool_name, arguments, status) VALUES (?, ?, ?, ?, ?, 'requested')")
            .bind(id).bind(call_id).bind(sequence).bind(name).bind(sqlx::types::Json(arguments)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Request one decision covering all approval-required capabilities of a call.
    /// The event log is the authoritative approval state; no separate grant survives
    /// this invocation or can authorize a later call.
    pub async fn request_approval(
        &self,
        id: &str,
        call_id: &str,
        capabilities: &[Capability],
    ) -> Result<()> {
        ensure!(
            !capabilities.is_empty(),
            "approval capabilities are required"
        );
        let unique: std::collections::HashSet<_> = capabilities.iter().collect();
        ensure!(
            unique.len() == capabilities.len(),
            "duplicate approval capability"
        );
        let mut tx = self.pool.begin().await?;
        self.lock_requested_tool(&mut tx, id, call_id).await?;
        ensure!(
            Self::approval_state(&mut tx, id, call_id).await?.is_none(),
            "approval already requested"
        );
        self.append_in(
            &mut tx,
            id,
            "approval.requested",
            &json!({"call_id": call_id, "capabilities": capabilities}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn decide_approval(&self, id: &str, call_id: &str, granted: bool) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.lock_requested_tool(&mut tx, id, call_id).await?;
        ensure!(
            Self::approval_state(&mut tx, id, call_id).await?.as_deref()
                == Some("approval.requested"),
            "approval is not pending"
        );
        let event_type = if granted {
            "approval.granted"
        } else {
            "approval.denied"
        };
        self.append_in(&mut tx, id, event_type, &json!({"call_id": call_id}))
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn lock_requested_tool(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
        call_id: &str,
    ) -> Result<()> {
        // A write first serializes concurrent requests and decisions before their
        // read of the append-only event log.
        let found: Option<String> = sqlx::query_scalar("UPDATE agent_runs SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND status = 'running' RETURNING id")
            .bind(id).bind(&self.workspace_id).fetch_optional(&mut **tx).await?;
        ensure!(
            found.is_some(),
            "run not found in workspace or already terminal"
        );
        let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tool_invocations WHERE run_id = ? AND call_id = ? AND status = 'requested')")
            .bind(id).bind(call_id).fetch_one(&mut **tx).await?;
        ensure!(pending, "tool invocation is not pending");
        Ok(())
    }

    async fn approval_state(
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
        call_id: &str,
    ) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT event_type FROM agent_events WHERE run_id = ? AND event_type IN ('approval.requested', 'approval.granted', 'approval.denied') AND json_extract(payload, '$.call_id') = ? ORDER BY sequence DESC LIMIT 1")
            .bind(id).bind(call_id).fetch_optional(&mut **tx).await?)
    }

    async fn deny_pending_approval(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
        call_id: &str,
        reason: &str,
    ) -> Result<()> {
        if Self::approval_state(tx, id, call_id).await?.as_deref() == Some("approval.requested") {
            self.append_in(
                tx,
                id,
                "approval.denied",
                &json!({"call_id": call_id, "reason": reason}),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn start_tool(&self, id: &str, call_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.append_in(&mut tx, id, "tool.started", &json!({"call_id": call_id}))
            .await?;
        let approval = Self::approval_state(&mut tx, id, call_id).await?;
        ensure!(
            approval.is_none() || approval.as_deref() == Some("approval.granted"),
            "tool approval is pending or denied"
        );
        let changed = sqlx::query("UPDATE tool_invocations SET status = 'started', started_at = ? WHERE run_id = ? AND call_id = ? AND status = 'requested'")
            .bind(Utc::now().timestamp_millis()).bind(id).bind(call_id).execute(&mut *tx).await?.rows_affected();
        ensure!(changed == 1, "tool invocation is not pending");
        tx.commit().await?;
        Ok(())
    }

    pub async fn finish_tool(&self, id: &str, call_id: &str, outcome: ToolOutcome) -> Result<()> {
        let (status, expected, result, error) = match outcome {
            ToolOutcome::Completed(result) => (
                "completed",
                "started",
                Some(sqlx::types::Json(result)),
                None,
            ),
            ToolOutcome::Failed(error) => ("failed", "started", None, Some(error)),
            ToolOutcome::Denied(error) => ("denied", "requested", None, Some(error)),
        };
        let mut tx = self.pool.begin().await?;
        self.append_in(
            &mut tx,
            id,
            &format!("tool.{status}"),
            &json!({"call_id": call_id, "error": error}),
        )
        .await?;
        let changed = sqlx::query("UPDATE tool_invocations SET status = ?, result = ?, error = ?, completed_at = ? WHERE run_id = ? AND call_id = ? AND status = ?")
            .bind(status).bind(result).bind(error).bind(Utc::now().timestamp_millis()).bind(id).bind(call_id).bind(expected).execute(&mut *tx).await?.rows_affected();
        ensure!(changed == 1, "invalid tool invocation transition");
        self.deny_pending_approval(&mut tx, id, call_id, "tool denied")
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn tool_invocations(&self, id: &str) -> Result<Vec<ToolInvocation>> {
        self.get_run(id)
            .await?
            .context("run not found in workspace")?;
        Ok(sqlx::query_as("SELECT t.* FROM tool_invocations t JOIN agent_runs r ON r.id = t.run_id WHERE t.run_id = ? AND r.workspace_id = ? ORDER BY t.requested_sequence")
            .bind(id).bind(&self.workspace_id).fetch_all(&self.pool).await?)
    }

    async fn append_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
        event_type: &str,
        payload: &Value,
    ) -> Result<i64> {
        let now = Utc::now().timestamp_millis();
        let sequence: i64 = sqlx::query_scalar("UPDATE agent_runs SET last_sequence = last_sequence + 1, updated_at = ? WHERE id = ? AND workspace_id = ? AND status = 'running' RETURNING last_sequence")
            .bind(now).bind(id).bind(&self.workspace_id).fetch_optional(&mut **tx).await?
            .context("run not found in workspace or already terminal")?;
        sqlx::query("INSERT INTO agent_events (run_id, sequence, timestamp, event_type, payload) VALUES (?, ?, ?, ?, ?)")
            .bind(id).bind(sequence).bind(now).bind(event_type).bind(sqlx::types::Json(payload)).execute(&mut **tx).await?;
        Ok(sequence)
    }
}
