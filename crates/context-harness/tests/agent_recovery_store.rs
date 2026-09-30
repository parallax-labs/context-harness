use context_harness::{
    agent_store::{AgentRunStore, RunOutcome, ToolOutcome, MAX_ARTIFACT_BYTES},
    app_store::SqliteAppStore,
    config::Config,
};
use serde_json::json;
use tempfile::TempDir;

async fn setup() -> (TempDir, SqliteAppStore, AgentRunStore) {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let app = SqliteAppStore::connect(&cfg).await.unwrap();
    let store = app.agent_runs("project").unwrap();
    (tmp, app, store)
}

#[tokio::test]
async fn resume_is_atomic_scoped_and_optimistically_guarded() {
    let (_tmp, app, store) = setup().await;
    for status in ["running", "failed", "cancelled"] {
        let run = store
            .create_run("agent", "version", "fake", "input")
            .await
            .unwrap();
        if status == "failed" {
            store
                .finish_run(&run.id, RunOutcome::Failed("network".into()))
                .await
                .unwrap();
        } else if status == "cancelled" {
            store
                .finish_run(&run.id, RunOutcome::Cancelled)
                .await
                .unwrap();
        }
        let before = store.get_run(&run.id).await.unwrap().unwrap();
        let other = app.agent_runs("other").unwrap();
        assert!(other
            .reopen_run(&run.id, before.last_sequence)
            .await
            .is_err());
        assert!(store
            .reopen_run(&run.id, before.last_sequence + 1)
            .await
            .is_err());
        let resumed = store
            .reopen_run(&run.id, before.last_sequence)
            .await
            .unwrap();
        assert_eq!(resumed.status, "running");
        assert_eq!(resumed.last_sequence, before.last_sequence + 1);
        assert!(
            resumed.error.is_none() && resumed.output.is_none() && resumed.completed_at.is_none()
        );
        let events = store
            .events(&run.id, before.last_sequence, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "run.resumed");
        assert_eq!(events[0].payload["previous_status"], status);
        assert!(store
            .reopen_run(&run.id, before.last_sequence)
            .await
            .is_err());
        store
            .finish_run(&run.id, RunOutcome::Completed("done".into()))
            .await
            .unwrap();
        let terminal = store.get_run(&run.id).await.unwrap().unwrap();
        assert!(store
            .reopen_run(&run.id, terminal.last_sequence)
            .await
            .is_err());
        assert_eq!(
            store.get_run(&run.id).await.unwrap().unwrap().last_sequence,
            terminal.last_sequence
        );
    }
}

#[tokio::test]
async fn uncertain_and_denied_tools_block_resume_without_mutation() {
    let (_tmp, _app, store) = setup().await;
    for status in ["requested", "started", "failed", "denied", "completed"] {
        let run = store
            .create_run("agent", "version", "fake", "input")
            .await
            .unwrap();
        store
            .request_tool(&run.id, "call", "process.exec", &json!({}))
            .await
            .unwrap();
        if ["started", "failed", "completed"].contains(&status) {
            store.start_tool(&run.id, "call").await.unwrap();
        }
        match status {
            "failed" => store
                .finish_tool(&run.id, "call", ToolOutcome::Failed("uncertain".into()))
                .await
                .unwrap(),
            "denied" => store
                .finish_tool(&run.id, "call", ToolOutcome::Denied("policy".into()))
                .await
                .unwrap(),
            "completed" => store
                .finish_tool(&run.id, "call", ToolOutcome::Completed(json!({"ok":true})))
                .await
                .unwrap(),
            _ => {}
        }
        let before = store.get_run(&run.id).await.unwrap().unwrap();
        let result = store.reopen_run(&run.id, before.last_sequence).await;
        assert_eq!(result.is_ok(), status == "completed");
        if status != "completed" {
            let after = store.get_run(&run.id).await.unwrap().unwrap();
            assert_eq!(after.last_sequence, before.last_sequence);
            assert_eq!(after.status, before.status);
        }
    }
}

#[tokio::test]
async fn artifact_metadata_is_validated_immutable_and_workspace_scoped() {
    let (tmp, app, store) = setup().await;
    let run = store
        .create_run("agent", "version", "fake", "input")
        .await
        .unwrap();
    let digest = "a".repeat(64);
    for path in [
        "",
        "/absolute",
        "../escape",
        "a/../escape",
        "./dot",
        "a//b",
        "a/",
        "a\\b",
        "C:/path",
        "a\nb",
    ] {
        assert!(
            store
                .record_artifact(&run.id, path, &digest, 1)
                .await
                .is_err(),
            "{path}"
        );
    }
    assert!(store
        .record_artifact(&run.id, "output.txt", "bad", 1)
        .await
        .is_err());
    assert!(store
        .record_artifact(&run.id, "output.txt", &digest, MAX_ARTIFACT_BYTES + 1)
        .await
        .is_err());
    assert!(store
        .append_event(&run.id, "artifact.created", &json!({}))
        .await
        .is_err());
    assert_eq!(
        store.get_run(&run.id).await.unwrap().unwrap().last_sequence,
        1
    );
    let seq = store
        .record_artifact(&run.id, "answers/final.txt", &digest, MAX_ARTIFACT_BYTES)
        .await
        .unwrap();
    assert!(store
        .record_artifact(&run.id, "answers/final.txt", &digest, 2)
        .await
        .is_err());
    let other = app.agent_runs("other").unwrap();
    assert!(other
        .record_artifact(&run.id, "other.txt", &digest, 0)
        .await
        .is_err());
    assert!(other.artifacts(&run.id).await.is_err());
    store
        .finish_run(&run.id, RunOutcome::Completed("done".into()))
        .await
        .unwrap();
    assert!(store
        .record_artifact(&run.id, "late.txt", &digest, 0)
        .await
        .is_err());
    app.close().await;
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    let reopened = SqliteAppStore::connect(&cfg).await.unwrap();
    let artifacts = reopened
        .agent_runs("project")
        .unwrap()
        .artifacts(&run.id)
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].sequence, seq);
    assert_eq!(artifacts[0].relative_path, "answers/final.txt");
    assert_eq!(artifacts[0].sha256, digest);
    assert_eq!(artifacts[0].size, MAX_ARTIFACT_BYTES);
}

#[tokio::test]
async fn concurrent_reopen_only_accepts_one_validation_snapshot() {
    let (_tmp, _app, store) = setup().await;
    let run = store
        .create_run("agent", "version", "fake", "input")
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        store.reopen_run(&run.id, run.last_sequence),
        store.reopen_run(&run.id, run.last_sequence)
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let events = store.events(&run.id, 0, 10).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "run.resumed")
            .count(),
        1
    );
}
