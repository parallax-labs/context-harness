//! Durable runtime history in the application's existing SQLite database.
//!
//! Every handle is bound to one workspace. State changes and their events commit
//! together. Event sequences are allocated by a write statement, avoiding a
//! read/modify/write race between concurrent writers. Timestamps are Unix ms.

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

    /// Append a runtime event. Run lifecycle and checkpoint events must use the
    /// dedicated operations so the event log and materialized state agree.
    pub async fn append_event(&self, id: &str, event_type: &str, payload: &Value) -> Result<i64> {
        ensure!(!event_type.trim().is_empty(), "event type is required");
        ensure!(
            !event_type.starts_with("run.") && !event_type.starts_with("checkpoint."),
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
