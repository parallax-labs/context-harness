use context_harness::agent_store::RunOutcome;
use context_harness::app_store::SqliteAppStore;
use context_harness::config::Config;
use serde_json::json;
use tempfile::TempDir;

fn config(tmp: &TempDir) -> Config {
    toml::from_str(&format!(
        "[db]\npath = {:?}\n[chunking]\nmax_tokens = 700\n[retrieval]\nfinal_limit = 12\n[server]\nbind = '127.0.0.1:0'\n",
        tmp.path().join("ctx.sqlite").to_str().unwrap()
    )).unwrap()
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
