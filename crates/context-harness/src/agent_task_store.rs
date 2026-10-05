//! Durable accepted agent work. Tasks own scheduling facts; linked runs own all
//! execution lifecycle, outcome, checkpoint, usage and effect history.

use crate::agent_store::{AgentRun, AgentRunStore, FailureReason, RunBudgets, RunOutcome};
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

const REQUEST_DOMAIN: &[u8] = b"context-harness.agent-task.request.v1\0";
pub const DEFAULT_QUEUE_LIMIT: u32 = 1_000;
pub const MAX_QUEUE_LIMIT: u32 = 10_000;
pub const MAX_TASK_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_PAYLOAD_IDENTITY_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Queued,
    Claimed,
    CancelRequested,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskSchedulingReason {
    CancelledBeforeRun,
    RunStopped,
    AcceptedIdentityChanged,
    ClaimAttemptsExhausted,
    RestartRequired,
    ReconciliationRequired,
    Unknown(String),
}

impl TaskSchedulingReason {
    pub fn from_stored(value: &str) -> Self {
        match value {
            "cancelled_before_run" => Self::CancelledBeforeRun,
            "run_stopped" => Self::RunStopped,
            "accepted_identity_changed" => Self::AcceptedIdentityChanged,
            "claim_attempts_exhausted" => Self::ClaimAttemptsExhausted,
            "restart_required" => Self::RestartRequired,
            "reconciliation_required" => Self::ReconciliationRequired,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

/// Non-secret, statically resolved execution identity accepted at submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptedTaskIdentity {
    value: Value,
    digest: String,
}

impl AcceptedTaskIdentity {
    pub fn new(value: Value) -> Result<Self> {
        ensure!(
            value.is_object(),
            "accepted task identity must be an object"
        );
        ensure!(
            value.get("schema_version").and_then(Value::as_u64) == Some(1),
            "unsupported accepted task identity schema"
        );
        let value = canonical_json(value);
        let encoded = serde_json::to_vec(&value)?;
        ensure!(
            encoded.len() <= 64 * 1024,
            "accepted task identity is too large"
        );
        Ok(Self {
            digest: format!("{:x}", Sha256::digest(&encoded)),
            value,
        })
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

fn canonical_json(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_json).collect()),
        other => other,
    }
}

#[derive(Debug, Clone)]
pub struct AgentTaskSubmission {
    pub request_key: String,
    pub agent_name: String,
    pub agent_version: String,
    pub input: String,
    pub payload_identity: Vec<u8>,
    pub accepted_identity: AcceptedTaskIdentity,
    pub queue_limit: u32,
}

#[derive(Debug, Serialize, FromRow)]
pub struct AgentTask {
    pub id: String,
    pub workspace_id: String,
    pub request_key: String,
    pub request_digest: String,
    pub agent_name: String,
    pub agent_version: String,
    pub accepted_identity: sqlx::types::Json<Value>,
    pub accepted_identity_digest: String,
    pub input: String,
    pub payload_identity: Vec<u8>,
    pub status: AgentTaskStatus,
    /// Raw stored value is retained so older readers preserve future reasons.
    pub scheduling_reason: Option<String>,
    pub scheduling_detail: Option<sqlx::types::Json<Value>>,
    pub claim_owner: Option<String>,
    pub claim_token: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub claim_attempts: i64,
    pub run_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub claimed_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub last_sequence: i64,
}

impl AgentTask {
    pub fn scheduling_reason_kind(&self) -> Option<TaskSchedulingReason> {
        self.scheduling_reason
            .as_deref()
            .map(TaskSchedulingReason::from_stored)
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct AgentTaskEvent {
    pub task_id: String,
    pub sequence: i64,
    pub timestamp: i64,
    pub event_type: String,
    pub payload: sqlx::types::Json<Value>,
}

#[derive(Debug, Serialize)]
pub struct AgentTaskClaim {
    pub task: AgentTask,
    pub worker_id: String,
    pub claim_token: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "disposition", content = "task", rename_all = "snake_case")]
pub enum AgentTaskSubmissionResult {
    Created(AgentTask),
    ExistingIdentical(AgentTask),
    RequestConflict,
    QueueFull,
}

#[derive(Clone)]
pub struct AgentTaskStore {
    pool: SqlitePool,
    workspace_id: String,
}

impl AgentTaskStore {
    pub fn new(pool: SqlitePool, workspace_id: &str) -> Result<Self> {
        ensure!(!workspace_id.trim().is_empty(), "workspace ID is required");
        Ok(Self {
            pool,
            workspace_id: workspace_id.to_owned(),
        })
    }

    pub async fn submit(
        &self,
        submission: AgentTaskSubmission,
    ) -> Result<AgentTaskSubmissionResult> {
        validate_submission(&submission)?;
        let request_digest = request_digest(
            &submission.agent_name,
            &submission.input,
            &submission.payload_identity,
        )?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool.begin().await?;
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO agent_tasks (id, workspace_id, request_key, request_digest, agent_name, agent_version, accepted_identity, accepted_identity_digest, input, payload_identity, status, created_at, updated_at) SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ? WHERE (SELECT COUNT(*) FROM agent_tasks WHERE workspace_id = ? AND status != 'terminal') < ? ON CONFLICT(workspace_id, request_key) DO NOTHING RETURNING id",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(&submission.request_key)
        .bind(&request_digest)
        .bind(&submission.agent_name)
        .bind(&submission.agent_version)
        .bind(sqlx::types::Json(submission.accepted_identity.value()))
        .bind(submission.accepted_identity.digest())
        .bind(&submission.input)
        .bind(&submission.payload_identity)
        .bind(now)
        .bind(now)
        .bind(&self.workspace_id)
        .bind(i64::from(submission.queue_limit))
        .fetch_optional(&mut *tx)
        .await?;

        if inserted.is_some() {
            append_event(
                &mut tx,
                &id,
                now,
                "task.submitted",
                &json!({
                    "request_digest": request_digest,
                    "accepted_identity_digest": submission.accepted_identity.digest(),
                    "agent": submission.agent_name,
                }),
            )
            .await?;
            tx.commit().await?;
            return Ok(AgentTaskSubmissionResult::Created(
                self.get(&id).await?.context("created task missing")?,
            ));
        }

        let existing: Option<(String, String, String)> = sqlx::query_as(
            "SELECT id, request_digest, accepted_identity_digest FROM agent_tasks WHERE workspace_id = ? AND request_key = ?",
        )
        .bind(&self.workspace_id)
        .bind(&submission.request_key)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        match existing {
            Some((id, stored_request, stored_identity))
                if stored_request == request_digest
                    && stored_identity == submission.accepted_identity.digest() =>
            {
                Ok(AgentTaskSubmissionResult::ExistingIdentical(
                    self.get(&id).await?.context("existing task missing")?,
                ))
            }
            Some(_) => Ok(AgentTaskSubmissionResult::RequestConflict),
            None => Ok(AgentTaskSubmissionResult::QueueFull),
        }
    }

    pub async fn get(&self, id: &str) -> Result<Option<AgentTask>> {
        Ok(
            sqlx::query_as("SELECT * FROM agent_tasks WHERE id = ? AND workspace_id = ?")
                .bind(id)
                .bind(&self.workspace_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn list(&self, limit: u32) -> Result<Vec<AgentTask>> {
        ensure!(
            (1..=1_000).contains(&limit),
            "task list limit must be 1-1000"
        );
        Ok(sqlx::query_as(
            "SELECT * FROM agent_tasks WHERE workspace_id = ? ORDER BY created_at DESC, id LIMIT ?",
        )
        .bind(&self.workspace_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn events(
        &self,
        id: &str,
        after_sequence: i64,
        limit: u32,
    ) -> Result<Vec<AgentTaskEvent>> {
        ensure!(after_sequence >= 0, "invalid task event cursor");
        ensure!(
            (1..=1_000).contains(&limit),
            "task event limit must be 1-1000"
        );
        self.get(id).await?.context("task not found in workspace")?;
        Ok(sqlx::query_as("SELECT e.* FROM agent_task_events e JOIN agent_tasks t ON t.id = e.task_id WHERE e.task_id = ? AND t.workspace_id = ? AND e.sequence > ? ORDER BY e.sequence LIMIT ?")
            .bind(id)
            .bind(&self.workspace_id)
            .bind(after_sequence)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Claim the oldest queued task using the database clock for its lease.
    pub async fn claim_next(
        &self,
        worker_id: &str,
        lease_seconds: u64,
        max_attempts: u32,
    ) -> Result<Option<AgentTaskClaim>> {
        validate_worker_id(worker_id)?;
        ensure!(
            (5..=3_600).contains(&lease_seconds),
            "lease duration must be 5-3600 seconds"
        );
        ensure!(
            (1..=100).contains(&max_attempts),
            "maximum claim attempts must be 1-100"
        );
        let claim_token = Uuid::new_v4().to_string();
        let mut tx = self.pool.begin().await?;
        let task: Option<AgentTask> = sqlx::query_as(
            "WITH candidate AS (SELECT id FROM agent_tasks WHERE workspace_id = ? AND status = 'queued' AND claim_attempts < ? ORDER BY created_at, id LIMIT 1) UPDATE agent_tasks SET status = 'claimed', claim_owner = ?, claim_token = ?, lease_expires_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) + ?, claim_attempts = claim_attempts + 1, claimed_at = CAST(unixepoch('subsec') * 1000 AS INTEGER), updated_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE id = (SELECT id FROM candidate) AND workspace_id = ? AND status = 'queued' RETURNING *",
        )
        .bind(&self.workspace_id)
        .bind(i64::from(max_attempts))
        .bind(worker_id)
        .bind(&claim_token)
        .bind(i64::try_from(lease_seconds)?.saturating_mul(1_000))
        .bind(&self.workspace_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(task) = task else {
            tx.commit().await?;
            return Ok(None);
        };
        append_event(
            &mut tx,
            &task.id,
            task.updated_at,
            "task.claimed",
            &json!({"worker_id": worker_id, "claim_attempt": task.claim_attempts}),
        )
        .await?;
        tx.commit().await?;
        let task = self.get(&task.id).await?.context("claimed task missing")?;
        Ok(Some(AgentTaskClaim {
            task,
            worker_id: worker_id.to_owned(),
            claim_token,
        }))
    }

    /// Extend an owned claim from the database's current time.
    pub async fn heartbeat(&self, claim: &AgentTaskClaim, lease_seconds: u64) -> Result<AgentTask> {
        ensure!(
            (5..=3_600).contains(&lease_seconds),
            "lease duration must be 5-3600 seconds"
        );
        let mut tx = self.pool.begin().await?;
        let timestamp: Option<i64> = sqlx::query_scalar(
            "UPDATE agent_tasks SET lease_expires_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) + ?, updated_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE id = ? AND workspace_id = ? AND claim_owner = ? AND claim_token = ? AND status IN ('claimed', 'cancel_requested') RETURNING updated_at",
        )
        .bind(i64::try_from(lease_seconds)?.saturating_mul(1_000))
        .bind(&claim.task.id)
        .bind(&self.workspace_id)
        .bind(&claim.worker_id)
        .bind(&claim.claim_token)
        .fetch_optional(&mut *tx)
        .await?;
        let timestamp = timestamp.context("task claim is no longer owned by this worker")?;
        append_event(
            &mut tx,
            &claim.task.id,
            timestamp,
            "task.heartbeat",
            &json!({}),
        )
        .await?;
        tx.commit().await?;
        self.get(&claim.task.id)
            .await?
            .context("heartbeat task missing")
    }

    /// Atomically create the task's sole root run and persist its immutable link.
    #[allow(clippy::too_many_arguments)]
    pub async fn link_run(
        &self,
        claim: &AgentTaskClaim,
        agent_version: &str,
        model: &str,
        budgets: RunBudgets,
    ) -> Result<AgentRun> {
        let mut tx = self.pool.begin().await?;
        let task: AgentTask = sqlx::query_as(
            "UPDATE agent_tasks SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND status = 'claimed' AND claim_owner = ? AND claim_token = ? AND run_id IS NULL RETURNING *",
        )
        .bind(&claim.task.id)
        .bind(&self.workspace_id)
        .bind(&claim.worker_id)
        .bind(&claim.claim_token)
        .fetch_optional(&mut *tx)
        .await?
        .context("task claim cannot create a run")?;
        let runs = AgentRunStore::new(self.pool.clone(), &self.workspace_id)?;
        let run_id = runs
            .create_root_run_in(
                &mut tx,
                &task.agent_name,
                agent_version,
                model,
                &task.input,
                &budgets,
            )
            .await?;
        let timestamp: i64 = sqlx::query_scalar(
            "UPDATE agent_tasks SET run_id = ?, updated_at = CAST(unixepoch('subsec') * 1000 AS INTEGER) WHERE id = ? AND workspace_id = ? AND status = 'claimed' AND claim_owner = ? AND claim_token = ? AND run_id IS NULL RETURNING updated_at",
        )
        .bind(&run_id)
        .bind(&claim.task.id)
        .bind(&self.workspace_id)
        .bind(&claim.worker_id)
        .bind(&claim.claim_token)
        .fetch_one(&mut *tx)
        .await?;
        append_event(
            &mut tx,
            &claim.task.id,
            timestamp,
            "task.run_linked",
            &json!({"run_id": run_id}),
        )
        .await?;
        tx.commit().await?;
        runs.get_run(&run_id).await?.context("linked run missing")
    }

    pub async fn finish_identity_changed(&self, claim: &AgentTaskClaim) -> Result<AgentTask> {
        self.finish_claim(claim, "accepted_identity_changed", false)
            .await
    }

    pub async fn finish_run_stopped(&self, claim: &AgentTaskClaim) -> Result<AgentTask> {
        self.finish_claim(claim, "run_stopped", true).await
    }

    pub(crate) async fn fail_linked_run_setup(
        &self,
        claim: &AgentTaskClaim,
        run_id: &str,
    ) -> Result<AgentTask> {
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agent_tasks WHERE id = ? AND workspace_id = ? AND status = 'claimed' AND claim_owner = ? AND claim_token = ? AND run_id = ?)",
        )
        .bind(&claim.task.id)
        .bind(&self.workspace_id)
        .bind(&claim.worker_id)
        .bind(&claim.claim_token)
        .bind(run_id)
        .fetch_one(&self.pool)
        .await?;
        ensure!(owned, "task claim no longer owns the linked run");
        AgentRunStore::new(self.pool.clone(), &self.workspace_id)?
            .finish_run(
                run_id,
                RunOutcome::FailedWithReason {
                    code: FailureReason::RecoveryRejected,
                    error: "runtime assembly failed".into(),
                },
            )
            .await?;
        self.finish_run_stopped(claim).await
    }

    async fn finish_claim(
        &self,
        claim: &AgentTaskClaim,
        reason: &str,
        require_run: bool,
    ) -> Result<AgentTask> {
        let mut tx = self.pool.begin().await?;
        let run_predicate = if require_run {
            "run_id IS NOT NULL"
        } else {
            "run_id IS NULL"
        };
        let statement = format!(
            "UPDATE agent_tasks SET status = 'terminal', scheduling_reason = ?, scheduling_detail = '{{}}', claim_owner = NULL, claim_token = NULL, lease_expires_at = NULL, updated_at = CAST(unixepoch('subsec') * 1000 AS INTEGER), completed_at = CAST(unixepoch('subsec') * 1000 AS INTEGER), last_sequence = last_sequence + 1 WHERE id = ? AND workspace_id = ? AND status IN ('claimed', 'cancel_requested') AND claim_owner = ? AND claim_token = ? AND {run_predicate} RETURNING last_sequence, updated_at"
        );
        let changed: Option<(i64, i64)> = sqlx::query_as(&statement)
            .bind(reason)
            .bind(&claim.task.id)
            .bind(&self.workspace_id)
            .bind(&claim.worker_id)
            .bind(&claim.claim_token)
            .fetch_optional(&mut *tx)
            .await?;
        let (sequence, timestamp) = changed.context("task claim cannot be finalized")?;
        sqlx::query("INSERT INTO agent_task_events (task_id, sequence, timestamp, event_type, payload) VALUES (?, ?, ?, 'task.terminal', ?)")
            .bind(&claim.task.id)
            .bind(sequence)
            .bind(timestamp)
            .bind(sqlx::types::Json(json!({"scheduling_reason": reason})))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get(&claim.task.id)
            .await?
            .context("finalized task missing")
    }

    /// Cancel accepted work only while it is still queued. Worker-owned
    /// cancellation is introduced with Phase 4B.
    pub async fn cancel_queued(&self, id: &str) -> Result<AgentTask> {
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool.begin().await?;
        let sequence: Option<i64> = sqlx::query_scalar("UPDATE agent_tasks SET status = 'terminal', scheduling_reason = 'cancelled_before_run', scheduling_detail = '{}', updated_at = ?, completed_at = ?, last_sequence = last_sequence + 1 WHERE id = ? AND workspace_id = ? AND status = 'queued' AND run_id IS NULL RETURNING last_sequence")
            .bind(now)
            .bind(now)
            .bind(id)
            .bind(&self.workspace_id)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(sequence) = sequence {
            sqlx::query("INSERT INTO agent_task_events (task_id, sequence, timestamp, event_type, payload) VALUES (?, ?, ?, 'task.terminal', ?)")
                .bind(id)
                .bind(sequence)
                .bind(now)
                .bind(sqlx::types::Json(json!({"scheduling_reason":"cancelled_before_run"})))
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return self.get(id).await?.context("cancelled task missing");
        }
        let task: AgentTask =
            sqlx::query_as("SELECT * FROM agent_tasks WHERE id = ? AND workspace_id = ?")
                .bind(id)
                .bind(&self.workspace_id)
                .fetch_optional(&mut *tx)
                .await?
                .context("task not found in workspace")?;
        ensure!(
            task.status == AgentTaskStatus::Terminal,
            "task is no longer queued"
        );
        tx.commit().await?;
        Ok(task)
    }
}

fn validate_worker_id(worker_id: &str) -> Result<()> {
    ensure!(
        !worker_id.is_empty() && worker_id.len() <= 128 && !worker_id.chars().any(char::is_control),
        "worker ID must contain 1-128 non-control UTF-8 bytes"
    );
    Ok(())
}

fn validate_submission(submission: &AgentTaskSubmission) -> Result<()> {
    ensure!(
        !submission.request_key.is_empty()
            && submission.request_key.len() <= 256
            && !submission.request_key.chars().any(char::is_control),
        "request key must contain 1-256 non-control UTF-8 bytes"
    );
    ensure!(
        !submission.agent_name.trim().is_empty(),
        "agent name is required"
    );
    ensure!(
        !submission.agent_version.trim().is_empty(),
        "agent version is required"
    );
    ensure!(
        !submission.input.trim().is_empty() && submission.input.len() <= MAX_TASK_INPUT_BYTES,
        "task input must contain 1-65536 bytes"
    );
    ensure!(
        submission.payload_identity.len() <= MAX_PAYLOAD_IDENTITY_BYTES,
        "task payload identity exceeds 8 KiB"
    );
    ensure!(
        (1..=MAX_QUEUE_LIMIT).contains(&submission.queue_limit),
        "queue limit must be 1-10000"
    );
    Ok(())
}

fn request_digest(agent: &str, input: &str, payload: &[u8]) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(REQUEST_DOMAIN);
    for bytes in [agent.as_bytes(), input.as_bytes(), payload] {
        hash.update(u64::try_from(bytes.len())?.to_be_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

async fn append_event(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    now: i64,
    event_type: &str,
    payload: &Value,
) -> Result<i64> {
    let sequence: i64 = sqlx::query_scalar(
        "UPDATE agent_tasks SET last_sequence = last_sequence + 1, updated_at = ? WHERE id = ? RETURNING last_sequence",
    )
    .bind(now)
    .bind(id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("INSERT INTO agent_task_events (task_id, sequence, timestamp, event_type, payload) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind(sequence)
        .bind(now)
        .bind(event_type)
        .bind(sqlx::types::Json(payload))
        .execute(&mut **tx)
        .await?;
    Ok(sequence)
}
