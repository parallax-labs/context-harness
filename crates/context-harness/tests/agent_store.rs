use context_harness::agent_store::{BlockedReason, RunLifecycle, RunOutcome, RunOutcomeKind};
use context_harness::app_store::SqliteAppStore;
use context_harness::config::Config;
use context_harness::{db, migrate};
use serde_json::json;
use tempfile::TempDir;

fn config(tmp: &TempDir) -> Config {
    toml::from_str(&format!(
        "[db]\npath = {:?}\n[chunking]\nmax_tokens = 700\n[retrieval]\nfinal_limit = 12\n[server]\nbind = '127.0.0.1:0'\n",
        tmp.path().join("ctx.sqlite").to_str().unwrap()
    )).unwrap()
}

#[tokio::test]
async fn legacy_run_rows_receive_deterministic_typed_state() {
    let tmp = TempDir::new().unwrap();
    let config = config(&tmp);
    let pool = db::connect(&config).await.unwrap();
    sqlx::query(
        "CREATE TABLE agent_runs (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, agent_name TEXT NOT NULL, agent_version TEXT NOT NULL, model TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled')), created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, completed_at INTEGER, input TEXT NOT NULL, output TEXT, error TEXT, last_sequence INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence >= 0))",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (id, status) in [
        ("active", "running"),
        ("done", "completed"),
        ("broken", "failed"),
        ("stopped", "cancelled"),
    ] {
        sqlx::query("INSERT INTO agent_runs (id, workspace_id, agent_name, agent_version, model, status, created_at, updated_at, input) VALUES (?, 'project', 'fixture', 'v1', 'fake', ?, 1, 1, 'input')")
            .bind(id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;

    migrate::run_migrations(&config).await.unwrap();
    migrate::run_migrations(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let runs = app.agent_runs("project").unwrap();

    let active = runs.get_run("active").await.unwrap().unwrap();
    assert_eq!(active.lifecycle, RunLifecycle::Active);
    assert_eq!(active.outcome, None);
    assert_eq!(active.reason_code, None);
    assert_eq!(active.reason_detail, None);
    assert_eq!(active.budgets.max_turns, None);
    assert_eq!(active.usage.model_turns, 0);
    assert_eq!(active.usage.total_tokens, None);

    for (id, outcome, reason) in [
        ("done", RunOutcomeKind::Completed, "legacy_completed"),
        ("broken", RunOutcomeKind::Failed, "legacy_failure"),
        ("stopped", RunOutcomeKind::Cancelled, "legacy_cancelled"),
    ] {
        let run = runs.get_run(id).await.unwrap().unwrap();
        assert_eq!(run.lifecycle, RunLifecycle::Terminal);
        assert_eq!(run.outcome, Some(outcome));
        assert_eq!(run.reason_code.as_deref(), Some(reason));
        assert_eq!(run.reason_detail.unwrap().0, json!({}));
    }
}

#[tokio::test]
async fn history_and_checkpoints_survive_reopen_and_repeated_migration() {
    let tmp = TempDir::new().unwrap();
    let config = config(&tmp);
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let runs = app.agent_runs("project").unwrap();
    let run = runs
        .create_run("researcher", "sha256:version", "fake", "question")
        .await
        .unwrap();
    assert_eq!(run.status, "running");
    assert_eq!(run.lifecycle, RunLifecycle::Active);
    assert_eq!(run.outcome, None);
    assert_eq!(run.last_sequence, 1);
    assert!(runs.latest_checkpoint(&run.id).await.unwrap().is_none());
    runs.append_event(&run.id, "model.responded", &json!({"text": "answer"}))
        .await
        .unwrap();
    runs.save_checkpoint(&run.id, 1, 0, &json!({"messages": []}))
        .await
        .unwrap();
    let state = json!({"model": "fake", "workspace": "project", "messages": [{"role": "assistant", "content": "answer"}], "permissions": {"allow": ["read_only"]}});
    let checkpoint_sequence = runs.save_checkpoint(&run.id, 1, 1, &state).await.unwrap();
    runs.finish_run(&run.id, RunOutcome::Completed("answer".into()))
        .await
        .unwrap();
    app.close().await;

    SqliteAppStore::initialize_config(&config).await.unwrap();
    let reopened = SqliteAppStore::connect(&config).await.unwrap();
    let runs = reopened.agent_runs("project").unwrap();
    let persisted = runs.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(persisted.status, "completed");
    assert_eq!(persisted.lifecycle, RunLifecycle::Terminal);
    assert_eq!(persisted.outcome, Some(RunOutcomeKind::Completed));
    assert_eq!(persisted.reason_code.as_deref(), Some("final_response"));
    assert_eq!(persisted.output.as_deref(), Some("answer"));
    assert_eq!(persisted.completed_at, Some(persisted.updated_at));
    assert_eq!(runs.history(10).await.unwrap().len(), 1);
    let checkpoint = runs.latest_checkpoint(&run.id).await.unwrap().unwrap();
    assert_eq!(checkpoint.sequence, checkpoint_sequence);
    assert_eq!(checkpoint.state.0, state);
    assert_eq!(checkpoint.turn, 1);
    let events = runs.events(&run.id, 0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "run.started",
            "model.responded",
            "checkpoint.created",
            "checkpoint.created",
            "run.completed"
        ]
    );
    assert_eq!(runs.events(&run.id, 2, 1).await.unwrap()[0].sequence, 3);
    assert!(runs
        .append_event(&run.id, "tool.started", &json!({}))
        .await
        .is_err());
    assert!(runs
        .save_checkpoint(&run.id, 1, 2, &json!({}))
        .await
        .is_err());
    assert!(runs
        .finish_run(&run.id, RunOutcome::Cancelled)
        .await
        .is_err());
    assert_eq!(runs.events(&run.id, 0, 100).await.unwrap().len(), 5);
}

#[tokio::test]
async fn suspended_outcome_is_typed_projects_failed_and_cannot_resume() {
    let tmp = TempDir::new().unwrap();
    let config = config(&tmp);
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let runs = app.agent_runs("project").unwrap();
    let run = runs
        .create_run("agent", "v1", "fake", "input")
        .await
        .unwrap();

    runs.finish_run(
        &run.id,
        RunOutcome::Blocked {
            code: BlockedReason::ExternalDependency,
            message: "fixture service is unavailable".into(),
        },
    )
    .await
    .unwrap();
    let blocked = runs.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(blocked.status, "failed");
    assert_eq!(blocked.lifecycle, RunLifecycle::Suspended);
    assert_eq!(blocked.outcome, Some(RunOutcomeKind::Blocked));
    assert_eq!(blocked.reason_code.as_deref(), Some("external_dependency"));
    assert_eq!(
        blocked.reason_detail.as_ref().unwrap().0["message"],
        "fixture service is unavailable"
    );
    assert!(runs
        .reopen_run(&run.id, blocked.last_sequence)
        .await
        .is_err());
    let events = runs.events(&run.id, 0, 10).await.unwrap();
    assert_eq!(events.last().unwrap().event_type, "run.blocked");
}

#[tokio::test]
async fn workspace_scope_and_lifecycle_cannot_be_bypassed() {
    let tmp = TempDir::new().unwrap();
    let config = config(&tmp);
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let owner = app.agent_runs("owner").unwrap();
    let other = app.agent_runs("other").unwrap();
    let run = owner
        .create_run("agent", "v1", "fake", "input")
        .await
        .unwrap();
    assert!(other.get_run(&run.id).await.unwrap().is_none());
    assert!(other.history(10).await.unwrap().is_empty());
    assert!(other.events(&run.id, 0, 10).await.is_err());
    assert!(other.latest_checkpoint(&run.id).await.is_err());
    assert!(other
        .append_event(&run.id, "tool.started", &json!({}))
        .await
        .is_err());
    assert!(other
        .save_checkpoint(&run.id, 1, 0, &json!({}))
        .await
        .is_err());
    assert!(other
        .finish_run(&run.id, RunOutcome::Cancelled)
        .await
        .is_err());
    assert!(owner
        .append_event(&run.id, "run.completed", &json!({}))
        .await
        .is_err());
    assert!(owner
        .save_checkpoint(&run.id, 0, -1, &json!({}))
        .await
        .is_err());
    assert_eq!(
        owner.get_run(&run.id).await.unwrap().unwrap().last_sequence,
        1
    );
    owner
        .finish_run(&run.id, RunOutcome::Failed("provider error".into()))
        .await
        .unwrap();
    let failed = owner.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(failed.error.as_deref(), Some("provider error"));
    assert_eq!(failed.status, "failed");
    let second = owner
        .create_run("agent", "v1", "fake", "input")
        .await
        .unwrap();
    owner
        .finish_run(&second.id, RunOutcome::Cancelled)
        .await
        .unwrap();
    assert_eq!(
        owner.get_run(&second.id).await.unwrap().unwrap().status,
        "cancelled"
    );
}

#[tokio::test]
async fn concurrent_appends_allocate_gapless_run_local_sequences() {
    let tmp = TempDir::new().unwrap();
    let config = config(&tmp);
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let runs = app.agent_runs("project").unwrap();
    let run = runs
        .create_run("agent", "v1", "fake", "input")
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for i in 0..20 {
        let runs = runs.clone();
        let id = run.id.clone();
        tasks.push(tokio::spawn(async move {
            runs.append_event(&id, "model.requested", &json!({"index": i}))
                .await
                .unwrap()
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let events = runs.events(&run.id, 0, 100).await.unwrap();
    assert_eq!(
        events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
        (1..=21).collect::<Vec<_>>()
    );
    let second = runs
        .create_run("agent", "v1", "fake", "other")
        .await
        .unwrap();
    assert_eq!(second.last_sequence, 1);
}

#[tokio::test]
async fn failed_event_insert_rolls_back_run_state() {
    let tmp = TempDir::new().unwrap();
    let config = config(&tmp);
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let runs = app.agent_runs("project").unwrap();
    let run = runs
        .create_run("agent", "v1", "fake", "input")
        .await
        .unwrap();
    // Inject a database failure after sequence allocation to exercise rollback.
    sqlx::query("CREATE TRIGGER reject_event BEFORE INSERT ON agent_events BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
        .execute(app.pool()).await.unwrap();
    assert!(runs
        .finish_run(&run.id, RunOutcome::Completed("answer".into()))
        .await
        .is_err());
    assert!(runs
        .save_checkpoint(&run.id, 1, 0, &json!({}))
        .await
        .is_err());
    let persisted = runs.get_run(&run.id).await.unwrap().unwrap();
    assert_eq!(persisted.status, "running");
    assert_eq!(persisted.last_sequence, 1);
    assert!(runs.latest_checkpoint(&run.id).await.unwrap().is_none());
    sqlx::query("DROP TRIGGER reject_event")
        .execute(app.pool())
        .await
        .unwrap();
    // A snapshot write can fail after its event was inserted. Neither the
    // event nor the allocated sequence may survive that failure.
    sqlx::query("CREATE TRIGGER reject_checkpoint BEFORE INSERT ON agent_checkpoints BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
        .execute(app.pool()).await.unwrap();
    assert!(runs
        .save_checkpoint(&run.id, 1, 0, &json!({}))
        .await
        .is_err());
    assert_eq!(
        runs.get_run(&run.id).await.unwrap().unwrap().last_sequence,
        1
    );
    assert_eq!(runs.events(&run.id, 0, 100).await.unwrap().len(), 1);
    sqlx::query("DROP TRIGGER reject_checkpoint")
        .execute(app.pool())
        .await
        .unwrap();
    runs.finish_run(&run.id, RunOutcome::Completed("answer".into()))
        .await
        .unwrap();
    assert_eq!(runs.events(&run.id, 0, 100).await.unwrap().len(), 2);
}
