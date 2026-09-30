use context_harness::{
    agent_resource::Capability,
    agent_store::{AgentRunStore, RunOutcome, ToolOutcome},
    app_store::SqliteAppStore,
    config::Config,
};
use serde_json::json;
use tempfile::TempDir;

async fn fixture() -> (TempDir, Config, SqliteAppStore, AgentRunStore, String) {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::minimal();
    config.db.path = tmp.path().join("ctx.sqlite");
    SqliteAppStore::initialize_config(&config).await.unwrap();
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let store = app.agent_runs("workspace").unwrap();
    let run = store
        .create_run("writer", "v1", "fake", "edit")
        .await
        .unwrap();
    store
        .request_tool(&run.id, "call", "file.write", &json!({"path":"a.txt"}))
        .await
        .unwrap();
    (tmp, config, app, store, run.id)
}

#[tokio::test]
async fn grant_is_durable_correlated_and_required_before_start() {
    let (_tmp, config, app, store, id) = fixture().await;
    store
        .request_approval(&id, "call", &[Capability::WorkspaceWrite])
        .await
        .unwrap();
    assert!(store.start_tool(&id, "call").await.is_err());
    assert!(store
        .request_approval(&id, "call", &[Capability::WorkspaceWrite])
        .await
        .is_err());
    store.decide_approval(&id, "call", true).await.unwrap();
    assert!(store.decide_approval(&id, "call", false).await.is_err());
    app.close().await;
    let app = SqliteAppStore::connect(&config).await.unwrap();
    let store = app.agent_runs("workspace").unwrap();
    store.start_tool(&id, "call").await.unwrap();
    assert!(store
        .request_approval(&id, "call", &[Capability::WorkspaceWrite])
        .await
        .is_err());
    store
        .finish_tool(&id, "call", ToolOutcome::Completed(json!({})))
        .await
        .unwrap();
    store
        .finish_run(&id, RunOutcome::Completed("done".into()))
        .await
        .unwrap();
    let events = store.events(&id, 0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        vec![
            "run.started",
            "tool.requested",
            "approval.requested",
            "approval.granted",
            "tool.started",
            "tool.completed",
            "run.completed"
        ]
    );
    assert_eq!(
        events[2].payload.0,
        json!({"call_id":"call", "capabilities":["workspace_write"]})
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.sequence, index as i64 + 1);
    }
    assert!(store.decide_approval(&id, "call", true).await.is_err());
}

#[tokio::test]
async fn invalid_requests_and_cross_workspace_access_never_append_events() {
    let (_tmp, _config, app, store, id) = fixture().await;
    assert!(store.decide_approval(&id, "call", true).await.is_err());
    assert!(store.request_approval(&id, "call", &[]).await.is_err());
    assert!(store
        .request_approval(
            &id,
            "call",
            &[Capability::WorkspaceWrite, Capability::WorkspaceWrite]
        )
        .await
        .is_err());
    assert!(store
        .request_approval(&id, "missing", &[Capability::WorkspaceWrite])
        .await
        .is_err());
    let other = app.agent_runs("other").unwrap();
    assert!(other
        .request_approval(&id, "call", &[Capability::WorkspaceWrite])
        .await
        .is_err());
    assert!(other.decide_approval(&id, "call", true).await.is_err());
    for event in ["approval.requested", "approval.granted", "approval.denied"] {
        assert!(store
            .append_event(&id, event, &json!({"call_id":"call"}))
            .await
            .is_err());
    }
    assert_eq!(store.events(&id, 0, 100).await.unwrap().len(), 2);
    store
        .request_approval(&id, "call", &[Capability::WorkspaceWrite])
        .await
        .unwrap();
    store.decide_approval(&id, "call", false).await.unwrap();
    assert!(store.start_tool(&id, "call").await.is_err());
    assert!(store.decide_approval(&id, "call", true).await.is_err());
    store
        .finish_tool(&id, "call", ToolOutcome::Denied("denied".into()))
        .await
        .unwrap();
    assert_eq!(store.events(&id, 0, 100).await.unwrap().len(), 5);
}

#[tokio::test]
async fn terminal_cleanup_denies_pending_approval_before_failing_tool() {
    for outcome in [RunOutcome::Cancelled, RunOutcome::Failed("timeout".into())] {
        let (_tmp, _config, _app, store, id) = fixture().await;
        store
            .request_approval(&id, "call", &[Capability::WorkspaceWrite])
            .await
            .unwrap();
        assert!(store
            .finish_run(&id, RunOutcome::Completed("premature".into()))
            .await
            .is_err());
        assert_eq!(store.events(&id, 0, 100).await.unwrap().len(), 3);
        store.finish_run(&id, outcome).await.unwrap();
        let events = store.events(&id, 0, 100).await.unwrap();
        assert_eq!(events[3].event_type, "approval.denied");
        assert_eq!(events[3].payload.0["reason"], "run interrupted");
        assert_eq!(events[4].event_type, "tool.failed");
        assert_eq!(
            store.tool_invocations(&id).await.unwrap()[0].status,
            "failed"
        );
        assert!(store.decide_approval(&id, "call", true).await.is_err());
        assert!(store
            .request_approval(&id, "call", &[Capability::WorkspaceWrite])
            .await
            .is_err());
        assert_eq!(store.events(&id, 0, 100).await.unwrap().len(), 6);
    }
}

#[tokio::test]
async fn concurrent_requests_and_decisions_allow_exactly_one_winner() {
    let (_tmp, _config, _app, store, id) = fixture().await;
    let (a, b) = tokio::join!(
        store.request_approval(&id, "call", &[Capability::WorkspaceWrite]),
        store.request_approval(&id, "call", &[Capability::WorkspaceWrite]),
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let (a, b) = tokio::join!(
        store.decide_approval(&id, "call", true),
        store.decide_approval(&id, "call", false),
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(store.events(&id, 0, 100).await.unwrap().len(), 4);
}

#[tokio::test]
async fn denying_tool_closes_pending_request() {
    let (_tmp, _config, _app, store, id) = fixture().await;
    store
        .request_approval(&id, "call", &[Capability::WorkspaceWrite])
        .await
        .unwrap();
    store
        .finish_tool(&id, "call", ToolOutcome::Denied("invalid".into()))
        .await
        .unwrap();
    let events = store.events(&id, 0, 100).await.unwrap();
    assert_eq!(events.last().unwrap().event_type, "approval.denied");
    assert!(store.decide_approval(&id, "call", true).await.is_err());
}
