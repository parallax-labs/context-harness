//! Durable runtime history in the application's existing SQLite database.
//!
//! Every handle is bound to one workspace. State changes and their events commit
//! together. Event sequences are allocated by a write statement, avoiding a
//! read/modify/write race between concurrent writers. Timestamps are Unix ms.

use crate::agent_resource::Capability;
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

#[derive(Debug)]
pub struct AccountingOverflow;

impl std::fmt::Display for AccountingOverflow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("run accounting overflow")
    }
}

impl std::error::Error for AccountingOverflow {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RunBudgets {
    pub max_turns: Option<u64>,
    pub timeout_seconds: Option<u64>,
    pub max_total_tokens: Option<u64>,
    pub max_tool_calls: Option<u64>,
}

impl RunBudgets {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.max_turns != Some(0)
                && self.timeout_seconds != Some(0)
                && self.max_total_tokens != Some(0)
                && self.max_tool_calls != Some(0),
            "run budgets must be positive when configured"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAccounting {
    Complete,
    Partial,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunUsage {
    pub model_turns: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub responses_with_usage: u64,
    pub responses_without_usage: u64,
    pub tool_calls: u64,
    pub token_accounting: TokenAccounting,
}

/// Materialized state of an execution. Inputs and outputs are plain text.
#[derive(Debug, Serialize, FromRow)]
pub struct AgentRun {
    pub id: String,
    pub workspace_id: String,
    pub agent_name: String,
    pub agent_version: String,
    pub model: String,
    pub status: String,
    pub lifecycle: RunLifecycle,
    pub outcome: Option<RunOutcomeKind>,
    pub reason_code: Option<String>,
    pub reason_detail: Option<sqlx::types::Json<Value>>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub input: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub last_sequence: i64,
    pub budgets: sqlx::types::Json<RunBudgets>,
    pub usage: sqlx::types::Json<RunUsage>,
    pub elapsed_ms: i64,
}

/// Immutable ancestry of a run. Legacy runs without a row are treated as roots.
#[derive(Debug, Serialize, FromRow)]
pub struct RunLineage {
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub root_run_id: String,
    pub depth: i64,
    pub parent_call_id: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum RunLifecycle {
    Active,
    Suspended,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum RunOutcomeKind {
    Completed,
    Blocked,
    NeedsUserInput,
    Failed,
    LimitExceeded,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedReason {
    ExternalDependency,
    EnvironmentUnavailable,
    PolicyRestriction,
    ResourceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedsUserInputReason {
    DecisionRequired,
    InformationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    ModelError,
    ModelRefusal,
    ModelOutputTruncated,
    ContentFiltered,
    InvalidModelResponse,
    ToolError,
    PermissionDenied,
    StorageError,
    AccountingError,
    RecoveryRejected,
    LegacyFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitReason {
    ModelTurns,
    Duration,
    TotalTokens,
    TokenUsageUnavailable,
    ToolCalls,
    ToolCallsPerTurn,
    CheckpointSize,
    OutputSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDecision {
    Allowed,
    Exceeded(LimitReason),
}

#[derive(FromRow)]
struct BudgetRow {
    id: String,
    budgets: sqlx::types::Json<RunBudgets>,
    usage: sqlx::types::Json<RunUsage>,
}

/// A stopped transition; callers cannot create arbitrary lifecycle strings.
pub enum RunOutcome {
    Completed(String),
    Failed(String),
    Cancelled,
    Blocked {
        code: BlockedReason,
        message: String,
    },
    NeedsUserInput {
        code: NeedsUserInputReason,
        question: String,
    },
    FailedWithReason {
        code: FailureReason,
        error: String,
    },
    LimitExceeded {
        code: LimitReason,
    },
}

struct StoppedRun {
    lifecycle: RunLifecycle,
    outcome: RunOutcomeKind,
    reason_code: String,
    reason_detail: Value,
    status: &'static str,
    output: Option<String>,
    error: Option<String>,
}

fn reason_code<T: Serialize>(code: T) -> String {
    serde_json::to_value(code)
        .expect("reason codes serialize")
        .as_str()
        .expect("reason codes are strings")
        .to_owned()
}

fn stopped(outcome: RunOutcome) -> Result<StoppedRun> {
    let stopped = match outcome {
        RunOutcome::Completed(output) => StoppedRun {
            lifecycle: RunLifecycle::Terminal,
            outcome: RunOutcomeKind::Completed,
            reason_code: "final_response".into(),
            reason_detail: json!({}),
            status: "completed",
            output: Some(output),
            error: None,
        },
        RunOutcome::Failed(error) => StoppedRun {
            lifecycle: RunLifecycle::Terminal,
            outcome: RunOutcomeKind::Failed,
            reason_code: "legacy_failure".into(),
            reason_detail: json!({}),
            status: "failed",
            output: None,
            error: Some(error),
        },
        RunOutcome::Cancelled => StoppedRun {
            lifecycle: RunLifecycle::Terminal,
            outcome: RunOutcomeKind::Cancelled,
            reason_code: "cancellation_requested".into(),
            reason_detail: json!({}),
            status: "cancelled",
            output: None,
            error: None,
        },
        RunOutcome::Blocked { code, message } => {
            ensure!(
                !message.trim().is_empty() && message.len() <= 8192,
                "invalid blocked message"
            );
            StoppedRun {
                lifecycle: RunLifecycle::Suspended,
                outcome: RunOutcomeKind::Blocked,
                reason_code: reason_code(code),
                reason_detail: json!({"message": message}),
                status: "failed",
                output: None,
                error: None,
            }
        }
        RunOutcome::NeedsUserInput { code, question } => {
            ensure!(
                !question.trim().is_empty() && question.len() <= 8192,
                "invalid user-input question"
            );
            StoppedRun {
                lifecycle: RunLifecycle::Suspended,
                outcome: RunOutcomeKind::NeedsUserInput,
                reason_code: reason_code(code),
                reason_detail: json!({"question": question}),
                status: "failed",
                output: None,
                error: None,
            }
        }
        RunOutcome::FailedWithReason { code, error } => StoppedRun {
            lifecycle: RunLifecycle::Terminal,
            outcome: RunOutcomeKind::Failed,
            reason_code: reason_code(code),
            reason_detail: json!({}),
            status: "failed",
            output: None,
            error: Some(error),
        },
        RunOutcome::LimitExceeded { code } => StoppedRun {
            lifecycle: RunLifecycle::Terminal,
            outcome: RunOutcomeKind::LimitExceeded,
            reason_code: reason_code(code),
            reason_detail: json!({}),
            status: "failed",
            output: None,
            error: Some(
                match code {
                    LimitReason::ModelTurns => "maximum model turns reached",
                    LimitReason::Duration => "execution timeout",
                    LimitReason::TotalTokens => "total token budget exceeded",
                    LimitReason::TokenUsageUnavailable => "token usage unavailable",
                    LimitReason::ToolCalls => "tool call budget exceeded",
                    LimitReason::ToolCallsPerTurn => "too many tool calls in one turn",
                    LimitReason::CheckpointSize => "checkpoint size limit exceeded",
                    LimitReason::OutputSize => "output size limit exceeded",
                }
                .into(),
            ),
        },
    };
    ensure!(
        serde_json::to_vec(&stopped.reason_detail)?.len() <= 8192,
        "run reason detail exceeds 8 KiB"
    );
    Ok(stopped)
}

fn reason_code_for_event(outcome: RunOutcomeKind) -> &'static str {
    match outcome {
        RunOutcomeKind::Completed => "completed",
        RunOutcomeKind::Blocked => "blocked",
        RunOutcomeKind::NeedsUserInput => "needs_user_input",
        RunOutcomeKind::Failed => "failed",
        RunOutcomeKind::LimitExceeded => "limit_exceeded",
        RunOutcomeKind::Cancelled => "cancelled",
    }
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

/// Metadata for an immutable file stored beneath a run's artifact directory.
#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub sequence: i64,
    pub relative_path: String,
    pub sha256: String,
    pub size: u64,
}

pub const MAX_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;

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
        self.create_run_with_budgets(
            agent_name,
            agent_version,
            model,
            input,
            RunBudgets::default(),
        )
        .await
    }

    pub async fn create_run_with_budgets(
        &self,
        agent_name: &str,
        agent_version: &str,
        model: &str,
        input: &str,
        budgets: RunBudgets,
    ) -> Result<AgentRun> {
        budgets.validate()?;
        ensure!(!agent_name.trim().is_empty(), "agent name is required");
        ensure!(
            !agent_version.trim().is_empty(),
            "agent version is required"
        );
        ensure!(!model.trim().is_empty(), "model is required");
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO agent_runs (id, workspace_id, agent_name, agent_version, model, status, created_at, updated_at, input, budgets) VALUES (?, ?, ?, ?, ?, 'running', ?, ?, ?, ?)")
            .bind(&id).bind(&self.workspace_id).bind(agent_name).bind(agent_version).bind(model).bind(now).bind(now).bind(input).bind(sqlx::types::Json(&budgets))
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO agent_run_lineage (run_id, root_run_id, depth) VALUES (?, ?, 0)")
            .bind(&id)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        self.append_in(
            &mut tx,
            &id,
            "run.started",
            &json!({"agent": agent_name, "model": model, "workspace_id": self.workspace_id, "budgets": budgets}),
        )
        .await?;
        tx.commit().await?;
        self.get_run(&id).await?.context("created run missing")
    }

    pub async fn lineage(&self, id: &str) -> Result<RunLineage> {
        if !self.has_lineage_table().await? {
            self.get_run(id)
                .await?
                .context("run not found in workspace")?;
            return Ok(RunLineage {
                run_id: id.to_owned(),
                parent_run_id: None,
                root_run_id: id.to_owned(),
                depth: 0,
                parent_call_id: None,
            });
        }
        sqlx::query_as("SELECT r.id AS run_id, l.parent_run_id, COALESCE(l.root_run_id, r.id) AS root_run_id, COALESCE(l.depth, 0) AS depth, l.parent_call_id FROM agent_runs r LEFT JOIN agent_run_lineage l ON l.run_id = r.id WHERE r.id = ? AND r.workspace_id = ?")
            .bind(id).bind(&self.workspace_id).fetch_optional(&self.pool).await?.context("run not found in workspace")
    }

    pub async fn children(&self, id: &str) -> Result<Vec<RunLineage>> {
        self.get_run(id)
            .await?
            .context("run not found in workspace")?;
        if !self.has_lineage_table().await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as("SELECT l.* FROM agent_run_lineage l JOIN agent_runs r ON r.id = l.run_id WHERE l.parent_run_id = ? AND r.workspace_id = ? ORDER BY r.created_at, r.id")
            .bind(id).bind(&self.workspace_id).fetch_all(&self.pool).await?)
    }

    async fn has_lineage_table(&self) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'agent_run_lineage')")
            .fetch_one(&self.pool).await?)
    }

    /// Bind exactly one child to a started delegation invocation atomically.
    pub async fn create_child_run(
        &self,
        parent_id: &str,
        call_id: &str,
        agent_name: &str,
        agent_version: &str,
        model: &str,
        input: &str,
    ) -> Result<AgentRun> {
        self.create_child_run_with_budgets(
            parent_id,
            call_id,
            agent_name,
            agent_version,
            model,
            input,
            RunBudgets::default(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_child_run_with_budgets(
        &self,
        parent_id: &str,
        call_id: &str,
        agent_name: &str,
        agent_version: &str,
        model: &str,
        input: &str,
        budgets: RunBudgets,
    ) -> Result<AgentRun> {
        budgets.validate()?;
        ensure!(
            !agent_name.trim().is_empty()
                && !agent_version.trim().is_empty()
                && !model.trim().is_empty(),
            "agent identity is required"
        );
        let mut tx = self.pool.begin().await?;
        let found: Option<String> = sqlx::query_scalar("UPDATE agent_runs SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND status = 'running' RETURNING id")
            .bind(parent_id).bind(&self.workspace_id).fetch_optional(&mut *tx).await?;
        ensure!(
            found.is_some(),
            "parent not found in workspace or already terminal"
        );
        let started: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tool_invocations WHERE run_id = ? AND call_id = ? AND tool_name = 'agent.invoke' AND status = 'started')")
            .bind(parent_id).bind(call_id).fetch_one(&mut *tx).await?;
        ensure!(started, "delegation invocation is not started");
        let lineage: Option<RunLineage> =
            sqlx::query_as("SELECT * FROM agent_run_lineage WHERE run_id = ?")
                .bind(parent_id)
                .fetch_optional(&mut *tx)
                .await?;
        let (root, depth) = lineage
            .map(|l| (l.root_run_id, l.depth + 1))
            .unwrap_or((parent_id.to_owned(), 1));
        ensure!(depth <= 4, "maximum delegation depth exceeded");
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();
        sqlx::query("INSERT INTO agent_runs (id, workspace_id, agent_name, agent_version, model, status, created_at, updated_at, input, budgets) VALUES (?, ?, ?, ?, ?, 'running', ?, ?, ?, ?)")
            .bind(&id).bind(&self.workspace_id).bind(agent_name).bind(agent_version).bind(model).bind(now).bind(now).bind(input).bind(sqlx::types::Json(&budgets)).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO agent_run_lineage (run_id, parent_run_id, root_run_id, depth, parent_call_id) VALUES (?, ?, ?, ?, ?)")
            .bind(&id).bind(parent_id).bind(&root).bind(depth).bind(call_id).execute(&mut *tx).await?;
        self.append_in(
            &mut tx,
            parent_id,
            "delegation.started",
            &json!({"call_id": call_id, "child_run_id": id}),
        )
        .await?;
        self.append_in(&mut tx, &id, "run.created", &json!({"agent": agent_name, "model": model, "workspace_id": self.workspace_id, "parent_run_id": parent_id, "root_run_id": root, "depth": depth, "budgets": budgets})).await?;
        tx.commit().await?;
        self.get_run(&id).await?.context("created child missing")
    }

    pub async fn get_run(&self, id: &str) -> Result<Option<AgentRun>> {
        Ok(
            sqlx::query_as("SELECT *, MAX(0, COALESCE(completed_at, CAST(unixepoch('subsec') * 1000 AS INTEGER)) - created_at) AS elapsed_ms FROM agent_runs WHERE id = ? AND workspace_id = ?")
                .bind(id)
                .bind(&self.workspace_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Most recent runs first, with a bounded result count.
    pub async fn history(&self, limit: u32) -> Result<Vec<AgentRun>> {
        Ok(sqlx::query_as(
            "SELECT *, MAX(0, COALESCE(completed_at, CAST(unixepoch('subsec') * 1000 AS INTEGER)) - created_at) AS elapsed_ms FROM agent_runs WHERE workspace_id = ? ORDER BY created_at DESC, id LIMIT ?",
        )
        .bind(&self.workspace_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn active_budget_chain(
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
        workspace_id: &str,
    ) -> Result<Vec<BudgetRow>> {
        let rows = sqlx::query_as(
            "WITH RECURSIVE chain(id) AS (SELECT ? UNION ALL SELECT l.parent_run_id FROM agent_run_lineage l JOIN chain c ON l.run_id = c.id WHERE l.parent_run_id IS NOT NULL) SELECT r.id, r.budgets, r.usage FROM chain c JOIN agent_runs r ON r.id = c.id WHERE r.workspace_id = ? AND r.status = 'running'",
        )
        .bind(id)
        .bind(workspace_id)
        .fetch_all(&mut **tx)
        .await?;
        ensure!(
            !rows.is_empty(),
            "run not found in workspace or already terminal"
        );
        Ok(rows)
    }

    async fn save_usage(
        tx: &mut Transaction<'_, Sqlite>,
        row: &BudgetRow,
        usage: &RunUsage,
    ) -> Result<()> {
        sqlx::query("UPDATE agent_runs SET usage = ? WHERE id = ?")
            .bind(sqlx::types::Json(usage))
            .bind(&row.id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Atomically reserve one model attempt for this run and its active ancestors.
    pub async fn record_model_requested(
        &self,
        id: &str,
        payload: &Value,
    ) -> Result<BudgetDecision> {
        let mut tx = self.pool.begin().await?;
        let rows = Self::active_budget_chain(&mut tx, id, &self.workspace_id).await?;
        if rows.iter().any(|row| {
            row.budgets.max_total_tokens.is_some() && row.usage.responses_without_usage > 0
        }) {
            return Ok(BudgetDecision::Exceeded(LimitReason::TokenUsageUnavailable));
        }
        if rows.iter().any(|row| {
            row.budgets
                .max_total_tokens
                .is_some_and(|limit| row.usage.total_tokens.is_some_and(|total| total >= limit))
        }) {
            return Ok(BudgetDecision::Exceeded(LimitReason::TotalTokens));
        }
        if rows.iter().any(|row| {
            row.budgets
                .max_turns
                .is_some_and(|limit| row.usage.model_turns >= limit)
        }) {
            return Ok(BudgetDecision::Exceeded(LimitReason::ModelTurns));
        }
        for row in &rows {
            let mut usage = row.usage.0.clone();
            usage.model_turns = usage.model_turns.checked_add(1).ok_or(AccountingOverflow)?;
            Self::save_usage(&mut tx, row, &usage).await?;
        }
        self.append_in(&mut tx, id, "model.requested", payload)
            .await?;
        tx.commit().await?;
        Ok(BudgetDecision::Allowed)
    }

    /// Record provider usage and debit the complete active ancestry atomically.
    pub async fn record_model_responded(
        &self,
        id: &str,
        payload: &Value,
        usage: Option<(u64, u64, u64)>,
    ) -> Result<BudgetDecision> {
        let mut tx = self.pool.begin().await?;
        let rows = Self::active_budget_chain(&mut tx, id, &self.workspace_id).await?;
        let mut decision = BudgetDecision::Allowed;
        for row in &rows {
            let mut next = row.usage.0.clone();
            match usage {
                Some((input, output, total)) => {
                    next.input_tokens = Some(
                        next.input_tokens
                            .unwrap_or(0)
                            .checked_add(input)
                            .ok_or(AccountingOverflow)?,
                    );
                    next.output_tokens = Some(
                        next.output_tokens
                            .unwrap_or(0)
                            .checked_add(output)
                            .ok_or(AccountingOverflow)?,
                    );
                    next.total_tokens = Some(
                        next.total_tokens
                            .unwrap_or(0)
                            .checked_add(total)
                            .ok_or(AccountingOverflow)?,
                    );
                    next.responses_with_usage = next
                        .responses_with_usage
                        .checked_add(1)
                        .ok_or(AccountingOverflow)?;
                }
                None => {
                    next.responses_without_usage = next
                        .responses_without_usage
                        .checked_add(1)
                        .ok_or(AccountingOverflow)?;
                }
            }
            next.token_accounting = match (
                next.responses_with_usage > 0,
                next.responses_without_usage > 0,
            ) {
                (true, false) => TokenAccounting::Complete,
                (true, true) => TokenAccounting::Partial,
                _ => TokenAccounting::Unavailable,
            };
            if row.budgets.max_total_tokens.is_some() && usage.is_none() {
                decision = BudgetDecision::Exceeded(LimitReason::TokenUsageUnavailable);
            } else if row
                .budgets
                .max_total_tokens
                .is_some_and(|limit| next.total_tokens.is_some_and(|total| total > limit))
            {
                decision = BudgetDecision::Exceeded(LimitReason::TotalTokens);
            }
            Self::save_usage(&mut tx, row, &next).await?;
        }
        self.append_in(&mut tx, id, "model.responded", payload)
            .await?;
        tx.commit().await?;
        Ok(decision)
    }

    /// Check an entire response batch before any ordinary call is persisted.
    pub async fn check_tool_call_budget(&self, id: &str, calls: usize) -> Result<BudgetDecision> {
        let calls = u64::try_from(calls).context("tool-call count overflow")?;
        let mut tx = self.pool.begin().await?;
        let rows = Self::active_budget_chain(&mut tx, id, &self.workspace_id).await?;
        let exceeded = rows.iter().any(|row| {
            row.budgets.max_tool_calls.is_some_and(|limit| {
                row.usage
                    .tool_calls
                    .checked_add(calls)
                    .is_none_or(|total| total > limit)
            })
        });
        tx.rollback().await?;
        Ok(if exceeded {
            BudgetDecision::Exceeded(LimitReason::ToolCalls)
        } else {
            BudgetDecision::Allowed
        })
    }

    pub async fn check_model_turn_budget(&self, id: &str) -> Result<BudgetDecision> {
        let mut tx = self.pool.begin().await?;
        let rows = Self::active_budget_chain(&mut tx, id, &self.workspace_id).await?;
        let decision = if rows.iter().any(|row| {
            row.budgets.max_total_tokens.is_some() && row.usage.responses_without_usage > 0
        }) {
            BudgetDecision::Exceeded(LimitReason::TokenUsageUnavailable)
        } else if rows.iter().any(|row| {
            row.budgets
                .max_total_tokens
                .is_some_and(|limit| row.usage.total_tokens.is_some_and(|total| total >= limit))
        }) {
            BudgetDecision::Exceeded(LimitReason::TotalTokens)
        } else if rows.iter().any(|row| {
            row.budgets
                .max_turns
                .is_some_and(|limit| row.usage.model_turns >= limit)
        }) {
            BudgetDecision::Exceeded(LimitReason::ModelTurns)
        } else {
            BudgetDecision::Allowed
        };
        tx.rollback().await?;
        Ok(decision)
    }

    /// Append a runtime event. Run, tool lifecycle and checkpoint events use the
    /// dedicated operations so the event log and materialized state agree.
    pub async fn append_event(&self, id: &str, event_type: &str, payload: &Value) -> Result<i64> {
        ensure!(!event_type.trim().is_empty(), "event type is required");
        ensure!(
            !event_type.starts_with("run.")
                && !event_type.starts_with("checkpoint.")
                && !event_type.starts_with("tool.")
                && !event_type.starts_with("approval.")
                && !event_type.starts_with("artifact.")
                && !event_type.starts_with("delegation."),
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

    /// Reopen only an unchanged, recoverable run. The caller must also hold the
    /// runtime's exclusive execution lock and validate its checkpoint before
    /// calling this method; the sequence guard prevents stale validation.
    pub async fn reopen_run(&self, id: &str, expected_sequence: i64) -> Result<AgentRun> {
        ensure!(expected_sequence > 0, "invalid expected run sequence");
        let mut tx = self.pool.begin().await?;
        let previous: Option<String> = sqlx::query_scalar("UPDATE agent_runs SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND last_sequence = ? AND status IN ('running', 'failed', 'cancelled') AND lifecycle != 'suspended' AND outcome IS NOT 'limit_exceeded' RETURNING status")
            .bind(id).bind(&self.workspace_id).bind(expected_sequence).fetch_optional(&mut *tx).await?;
        let previous = previous
            .context("run not found in workspace, completed, or changed since validation")?;
        let delegated: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_run_lineage WHERE (run_id = ? AND parent_run_id IS NOT NULL) OR parent_run_id = ?)")
            .bind(id).bind(id).fetch_one(&mut *tx).await?;
        ensure!(
            !delegated,
            "cannot resume delegated child or parent with descendants"
        );
        let unsafe_tools: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tool_invocations WHERE run_id = ? AND status != 'completed')")
            .bind(id).fetch_one(&mut *tx).await?;
        ensure!(
            !unsafe_tools,
            "cannot resume run with incomplete, failed, or denied tool invocations"
        );
        sqlx::query("UPDATE agent_runs SET status = 'running', lifecycle = 'active', outcome = NULL, reason_code = NULL, reason_detail = NULL, output = NULL, error = NULL, completed_at = NULL WHERE id = ? AND workspace_id = ?")
            .bind(id).bind(&self.workspace_id).execute(&mut *tx).await?;
        self.append_in(
            &mut tx,
            id,
            "run.resumed",
            &json!({"previous_status": previous, "previous_sequence": expected_sequence}),
        )
        .await?;
        let run = sqlx::query_as("SELECT *, MAX(0, CAST(unixepoch('subsec') * 1000 AS INTEGER) - created_at) AS elapsed_ms FROM agent_runs WHERE id = ? AND workspace_id = ?")
            .bind(id)
            .bind(&self.workspace_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(run)
    }

    /// Record metadata after the caller durably publishes the artifact file.
    /// Paths are normalized relative to the workspace root. File
    /// contents and filesystem containment are validated by the runtime.
    pub async fn record_artifact(
        &self,
        id: &str,
        relative_path: &str,
        sha256: &str,
        size: u64,
    ) -> Result<i64> {
        ensure!(
            !relative_path.is_empty()
                && relative_path.len() <= 1024
                && !relative_path.contains(['\\', ':'])
                && !relative_path.chars().any(char::is_control)
                && relative_path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
            "artifact path must be normalized and relative"
        );
        ensure!(
            sha256.len() == 64
                && sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid artifact SHA256"
        );
        ensure!(size <= MAX_ARTIFACT_BYTES, "artifact exceeds size limit");
        let mut tx = self.pool.begin().await?;
        // Acquire the write lock before checking for a conflicting path.
        let found: Option<String> = sqlx::query_scalar("UPDATE agent_runs SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND status = 'running' RETURNING id")
            .bind(id).bind(&self.workspace_id).fetch_optional(&mut *tx).await?;
        ensure!(
            found.is_some(),
            "run not found in workspace or already terminal"
        );
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_events WHERE run_id = ? AND event_type = 'artifact.created' AND json_extract(payload, '$.relative_path') = ?)")
            .bind(id).bind(relative_path).fetch_one(&mut *tx).await?;
        ensure!(!exists, "artifact path already recorded");
        let sequence = self
            .append_in(
                &mut tx,
                id,
                "artifact.created",
                &json!({"relative_path": relative_path, "sha256": sha256, "size": size}),
            )
            .await?;
        tx.commit().await?;
        Ok(sequence)
    }

    pub async fn artifacts(&self, id: &str) -> Result<Vec<ArtifactMetadata>> {
        self.get_run(id)
            .await?
            .context("run not found in workspace")?;
        let events: Vec<AgentEvent> = sqlx::query_as("SELECT e.* FROM agent_events e JOIN agent_runs r ON r.id = e.run_id WHERE e.run_id = ? AND r.workspace_id = ? AND e.event_type = 'artifact.created' ORDER BY e.sequence")
            .bind(id).bind(&self.workspace_id).fetch_all(&self.pool).await?;
        events
            .into_iter()
            .map(|event| {
                let mut payload = event.payload.0;
                payload["sequence"] = json!(event.sequence);
                serde_json::from_value(payload).context("invalid artifact metadata")
            })
            .collect()
    }

    /// Terminal runs reject ordinary writes; recovery requires `reopen_run`.
    pub async fn finish_run(&self, id: &str, outcome: RunOutcome) -> Result<()> {
        let stopped = stopped(outcome)?;
        let mut tx = self.pool.begin().await?;
        // Acquire the write lock before reading invocation state. Terminal state
        // and cleanup of interrupted calls commit in the same transaction.
        let found: Option<String> = sqlx::query_scalar("UPDATE agent_runs SET updated_at = updated_at WHERE id = ? AND workspace_id = ? AND status = 'running' RETURNING id")
            .bind(id).bind(&self.workspace_id).fetch_optional(&mut *tx).await?;
        ensure!(
            found.is_some(),
            "run not found in workspace or already terminal"
        );
        let descendants: Vec<String> = sqlx::query_scalar("WITH RECURSIVE descendants(id) AS (SELECT run_id FROM agent_run_lineage WHERE parent_run_id = ? UNION ALL SELECT l.run_id FROM agent_run_lineage l JOIN descendants d ON l.parent_run_id = d.id) SELECT r.id FROM descendants d JOIN agent_runs r ON r.id = d.id WHERE r.workspace_id = ? AND r.status = 'running'")
            .bind(id).bind(&self.workspace_id).fetch_all(&mut *tx).await?;
        ensure!(
            stopped.outcome != RunOutcomeKind::Completed || descendants.is_empty(),
            "cannot complete run with active descendants"
        );
        for child in descendants {
            let child_outcome = if stopped.outcome == RunOutcomeKind::Cancelled {
                StoppedRun {
                    lifecycle: RunLifecycle::Terminal,
                    outcome: RunOutcomeKind::Cancelled,
                    reason_code: "ancestor_cancelled".into(),
                    reason_detail: json!({}),
                    status: "cancelled",
                    output: None,
                    error: None,
                }
            } else {
                StoppedRun {
                    lifecycle: RunLifecycle::Terminal,
                    outcome: RunOutcomeKind::Failed,
                    reason_code: "tool_error".into(),
                    reason_detail: json!({}),
                    status: "failed",
                    output: None,
                    error: Some("ancestor run interrupted".into()),
                }
            };
            self.finish_in(&mut tx, &child, &child_outcome).await?;
        }
        self.finish_in(&mut tx, id, &stopped).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn finish_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
        stopped: &StoppedRun,
    ) -> Result<()> {
        let state: (sqlx::types::Json<RunBudgets>, sqlx::types::Json<RunUsage>) =
            sqlx::query_as("SELECT budgets, usage FROM agent_runs WHERE id = ?")
                .bind(id)
                .fetch_one(&mut **tx)
                .await?;
        let unfinished: Vec<String> = sqlx::query_scalar("SELECT call_id FROM tool_invocations WHERE run_id = ? AND status IN ('requested', 'started') ORDER BY requested_sequence")
            .bind(id).fetch_all(&mut **tx).await?;
        ensure!(
            stopped.status != "completed" || unfinished.is_empty(),
            "cannot complete run with unfinished tools"
        );
        for call_id in unfinished {
            self.deny_pending_approval(tx, id, &call_id, "run interrupted")
                .await?;
            self.append_in(
                tx,
                id,
                "tool.failed",
                &json!({"call_id": call_id, "error": "run interrupted"}),
            )
            .await?;
            sqlx::query("UPDATE tool_invocations SET status = 'failed', error = 'run interrupted', completed_at = ? WHERE run_id = ? AND call_id = ?")
                .bind(Utc::now().timestamp_millis()).bind(id).bind(call_id).execute(&mut **tx).await?;
        }
        self.append_in(
            tx,
            id,
            &format!("run.{}", reason_code_for_event(stopped.outcome)),
            &json!({"lifecycle": stopped.lifecycle, "outcome": stopped.outcome, "reason_code": stopped.reason_code, "reason_detail": stopped.reason_detail, "usage": state.1, "budgets": state.0, "output": stopped.output, "error": stopped.error}),
        )
        .await?;
        sqlx::query("UPDATE agent_runs SET status = ?, lifecycle = ?, outcome = ?, reason_code = ?, reason_detail = ?, output = ?, error = ?, completed_at = updated_at WHERE id = ? AND workspace_id = ?")
            .bind(stopped.status).bind(stopped.lifecycle).bind(stopped.outcome).bind(&stopped.reason_code).bind(sqlx::types::Json(&stopped.reason_detail)).bind(&stopped.output).bind(&stopped.error).bind(id).bind(&self.workspace_id).execute(&mut **tx).await?;
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
        let rows = Self::active_budget_chain(&mut tx, id, &self.workspace_id).await?;
        ensure!(
            !rows.iter().any(|row| row
                .budgets
                .max_tool_calls
                .is_some_and(|limit| row.usage.tool_calls >= limit)),
            "tool-call budget exhausted"
        );
        let approval = Self::approval_state(&mut tx, id, call_id).await?;
        ensure!(
            approval.is_none() || approval.as_deref() == Some("approval.granted"),
            "tool approval is pending or denied"
        );
        let changed = sqlx::query("UPDATE tool_invocations SET status = 'started', started_at = ? WHERE run_id = ? AND call_id = ? AND status = 'requested'")
            .bind(Utc::now().timestamp_millis()).bind(id).bind(call_id).execute(&mut *tx).await?.rows_affected();
        ensure!(changed == 1, "tool invocation is not pending");
        for row in &rows {
            let mut usage = row.usage.0.clone();
            usage.tool_calls = usage.tool_calls.checked_add(1).ok_or(AccountingOverflow)?;
            Self::save_usage(&mut tx, row, &usage).await?;
        }
        self.append_in(&mut tx, id, "tool.started", &json!({"call_id": call_id}))
            .await?;
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
