use context_harness::{
    agent_store::RunBudgets,
    agent_task_store::{
        AcceptedTaskIdentity, AgentTaskClaim, AgentTaskStatus, AgentTaskStore, AgentTaskSubmission,
        AgentTaskSubmissionResult, TaskSchedulingReason,
    },
    app_store::SqliteAppStore,
    config::Config,
};
use serde_json::json;
use tempfile::TempDir;

async fn setup() -> (TempDir, SqliteAppStore, AgentTaskStore) {
    let temp = TempDir::new().unwrap();
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let store = app.agent_tasks("workspace").unwrap();
    (temp, app, store)
}

fn submission(key: &str, input: &str) -> AgentTaskSubmission {
    AgentTaskSubmission {
        request_key: key.into(),
        agent_name: "fixture".into(),
        agent_version: "agent-v1".into(),
        input: input.into(),
        payload_identity: b"host-job-v1".to_vec(),
        accepted_identity: AcceptedTaskIdentity::new(json!({
            "schema_version": 1,
            "workspace_id": "workspace",
            "agent": "fixture"
        }))
        .unwrap(),
        queue_limit: 1000,
    }
}

fn created(result: AgentTaskSubmissionResult) -> context_harness::agent_task_store::AgentTask {
    match result {
        AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    }
}

#[test]
fn accepted_identity_digest_uses_canonical_key_order() {
    let first = AcceptedTaskIdentity::new(json!({
        "schema_version": 1,
        "nested": {"z": 1, "a": 2}
    }))
    .unwrap();
    let second: serde_json::Value =
        serde_json::from_str(r#"{"nested":{"a":2,"z":1},"schema_version":1}"#).unwrap();
    let second = AcceptedTaskIdentity::new(second).unwrap();
    assert_eq!(first.digest(), second.digest());
}

#[tokio::test]
async fn submission_is_idempotent_conflict_aware_and_durable() {
    let (temp, app, store) = setup().await;
    let first = created(
        store
            .submit(submission("request-1", "input"))
            .await
            .unwrap(),
    );
    assert_eq!(first.status, AgentTaskStatus::Queued);
    assert_eq!(first.last_sequence, 1);
    assert_eq!(first.payload_identity, b"host-job-v1");

    let duplicate = store
        .submit(submission("request-1", "input"))
        .await
        .unwrap();
    let AgentTaskSubmissionResult::ExistingIdentical(duplicate) = duplicate else {
        panic!("expected identical submission")
    };
    assert_eq!(duplicate.id, first.id);
    assert_eq!(duplicate.last_sequence, 1);

    assert!(matches!(
        store
            .submit(submission("request-1", "different"))
            .await
            .unwrap(),
        AgentTaskSubmissionResult::RequestConflict
    ));
    let mut changed_identity = submission("request-1", "input");
    changed_identity.accepted_identity =
        AcceptedTaskIdentity::new(json!({"schema_version":1,"changed":true})).unwrap();
    assert!(matches!(
        store.submit(changed_identity).await.unwrap(),
        AgentTaskSubmissionResult::RequestConflict
    ));

    let events = store.events(&first.id, 0, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "task.submitted");
    assert_eq!(events[0].sequence, 1);
    assert_eq!(store.list(10).await.unwrap().len(), 1);

    app.close().await;
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let reopened = SqliteAppStore::connect(&config).await.unwrap();
    assert_eq!(
        reopened
            .agent_tasks("workspace")
            .unwrap()
            .get(&first.id)
            .await
            .unwrap()
            .unwrap()
            .request_digest,
        first.request_digest
    );
}

#[tokio::test]
async fn queue_bounds_and_workspace_scope_are_enforced() {
    let (_temp, app, store) = setup().await;
    let mut first = submission("one", "one");
    first.queue_limit = 1;
    let first = created(store.submit(first).await.unwrap());
    let mut second = submission("two", "two");
    second.queue_limit = 1;
    assert!(matches!(
        store.submit(second).await.unwrap(),
        AgentTaskSubmissionResult::QueueFull
    ));
    let other = app.agent_tasks("other").unwrap();
    assert!(other.get(&first.id).await.unwrap().is_none());
    assert!(other.events(&first.id, 0, 10).await.is_err());
    assert!(other.cancel_queued(&first.id).await.is_err());
    assert!(matches!(
        other
            .submit(submission("one", "other workspace"))
            .await
            .unwrap(),
        AgentTaskSubmissionResult::Created(_)
    ));
}

#[tokio::test]
async fn queued_cancellation_is_atomic_idempotent_and_frees_capacity() {
    let (_temp, _app, store) = setup().await;
    let mut initial = submission("one", "one");
    initial.queue_limit = 1;
    let task = created(store.submit(initial).await.unwrap());
    let cancelled = store.cancel_queued(&task.id).await.unwrap();
    assert_eq!(cancelled.status, AgentTaskStatus::Terminal);
    assert_eq!(
        cancelled.scheduling_reason_kind(),
        Some(TaskSchedulingReason::CancelledBeforeRun)
    );
    assert_eq!(cancelled.last_sequence, 2);
    assert!(cancelled.completed_at.is_some());
    assert_eq!(
        store.cancel_queued(&task.id).await.unwrap().last_sequence,
        2
    );
    let events = store.events(&task.id, 0, 10).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        ["task.submitted", "task.terminal"]
    );

    let mut replacement = submission("two", "two");
    replacement.queue_limit = 1;
    assert!(matches!(
        store.submit(replacement).await.unwrap(),
        AgentTaskSubmissionResult::Created(_)
    ));
}

#[tokio::test]
async fn inspection_preserves_unknown_future_scheduling_reasons() {
    let (_temp, app, store) = setup().await;
    let task = created(store.submit(submission("future", "input")).await.unwrap());
    sqlx::query(
        "UPDATE agent_tasks SET status = 'terminal', scheduling_reason = 'future_reason' WHERE id = ?",
    )
    .bind(&task.id)
    .execute(app.pool())
    .await
    .unwrap();
    let inspected = store.get(&task.id).await.unwrap().unwrap();
    assert_eq!(
        inspected.scheduling_reason.as_deref(),
        Some("future_reason")
    );
    assert_eq!(
        inspected.scheduling_reason_kind(),
        Some(TaskSchedulingReason::Unknown("future_reason".into()))
    );
}

#[tokio::test]
async fn concurrent_identical_submission_returns_one_task() {
    let (_temp, _app, store) = setup().await;
    let (left, right) = tokio::join!(
        store.submit(submission("same", "input")),
        store.submit(submission("same", "input"))
    );
    let left = left.unwrap();
    let right = right.unwrap();
    let ids = [left, right]
        .into_iter()
        .map(|result| match result {
            AgentTaskSubmissionResult::Created(task)
            | AgentTaskSubmissionResult::ExistingIdentical(task) => task.id,
            _ => panic!("unexpected concurrent submission result"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ids[0], ids[1]);
    assert_eq!(store.list(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_inputs_fail_before_writes() {
    let (_temp, _app, store) = setup().await;
    let mut invalid = submission("", "input");
    assert!(store.submit(invalid.clone()).await.is_err());
    invalid.request_key = "key".into();
    invalid.input = " ".into();
    assert!(store.submit(invalid.clone()).await.is_err());
    invalid.input = "input".into();
    invalid.queue_limit = 0;
    assert!(store.submit(invalid.clone()).await.is_err());
    invalid.queue_limit = 1;
    invalid.payload_identity = vec![0; 8193];
    assert!(store.submit(invalid).await.is_err());
    assert!(store.list(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn claims_are_exclusive_database_timed_and_token_guarded() {
    let (_temp, app, store) = setup().await;
    let task = created(store.submit(submission("claim", "input")).await.unwrap());
    let other = store.clone();
    let (left, right) = tokio::join!(
        store.claim_next("worker-a", 60, 3),
        other.claim_next("worker-b", 60, 3)
    );
    let mut claims = [left.unwrap(), right.unwrap()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(claims.len(), 1);
    let first = claims.pop().unwrap();
    assert_eq!(first.task.id, task.id);
    assert_eq!(first.task.status, AgentTaskStatus::Claimed);
    assert_eq!(first.task.claim_attempts, 1);
    assert!(first.task.lease_expires_at.unwrap() > first.task.updated_at);
    assert!(store.claim_next("worker-c", 60, 3).await.unwrap().is_none());

    let stale = AgentTaskClaim {
        task: store.get(&task.id).await.unwrap().unwrap(),
        worker_id: first.worker_id.clone(),
        claim_token: "stale-token".into(),
    };
    assert!(store.heartbeat(&stale, 60).await.is_err());
    let renewed = store.heartbeat(&first, 60).await.unwrap();
    assert_eq!(renewed.last_sequence, 3);
    assert_eq!(
        store
            .events(&task.id, 0, 10)
            .await
            .unwrap()
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        ["task.submitted", "task.claimed", "task.heartbeat"]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_runs")
            .fetch_one(app.pool())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn run_creation_and_task_link_are_atomic_and_immutable() {
    let (_temp, app, store) = setup().await;
    let task = created(store.submit(submission("link", "input")).await.unwrap());
    let claim = store.claim_next("worker", 60, 3).await.unwrap().unwrap();
    let stale = AgentTaskClaim {
        task: store.get(&task.id).await.unwrap().unwrap(),
        worker_id: claim.worker_id.clone(),
        claim_token: "stale-token".into(),
    };
    assert!(store
        .link_run(&stale, "agent-v1", "fake", RunBudgets::default())
        .await
        .is_err());
    assert!(store.finish_identity_changed(&stale).await.is_err());
    let run = store
        .link_run(&claim, "agent-v1", "fake", RunBudgets::default())
        .await
        .unwrap();
    assert_eq!(run.input, "input");
    assert_eq!(
        store
            .get(&task.id)
            .await
            .unwrap()
            .unwrap()
            .run_id
            .as_deref(),
        Some(run.id.as_str())
    );
    assert!(store
        .link_run(&claim, "agent-v1", "fake", RunBudgets::default())
        .await
        .is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_runs")
            .fetch_one(app.pool())
            .await
            .unwrap(),
        1
    );
    let terminal = store.finish_run_stopped(&claim).await.unwrap();
    assert_eq!(terminal.status, AgentTaskStatus::Terminal);
    assert_eq!(terminal.run_id.as_deref(), Some(run.id.as_str()));
    assert!(terminal.claim_token.is_none());
}

#[tokio::test]
async fn identity_drift_finishes_without_creating_a_run() {
    let (_temp, app, store) = setup().await;
    let task = created(store.submit(submission("drift", "input")).await.unwrap());
    let claim = store.claim_next("worker", 60, 3).await.unwrap().unwrap();
    let terminal = store.finish_identity_changed(&claim).await.unwrap();
    assert_eq!(
        terminal.scheduling_reason_kind(),
        Some(TaskSchedulingReason::AcceptedIdentityChanged)
    );
    assert!(terminal.run_id.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_runs")
            .fetch_one(app.pool())
            .await
            .unwrap(),
        0
    );
    assert_eq!(store.get(&task.id).await.unwrap().unwrap().last_sequence, 3);
}
