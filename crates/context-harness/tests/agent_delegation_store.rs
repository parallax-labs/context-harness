use context_harness::{
    agent_resource::Capability,
    agent_store::{AgentRunStore, RunOutcome, ToolOutcome},
    app_store::SqliteAppStore,
    config::Config,
};
use serde_json::json;
use tempfile::TempDir;

async fn setup() -> (TempDir, Config, SqliteAppStore, AgentRunStore) {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let app = SqliteAppStore::connect(&cfg).await.unwrap();
    let store = app.agent_runs("one").unwrap();
    (tmp, cfg, app, store)
}
async fn start(store: &AgentRunStore, parent: &str, call: &str) {
    store
        .request_tool(parent, call, "agent.invoke", &json!({"agent":"child"}))
        .await
        .unwrap();
    store.start_tool(parent, call).await.unwrap();
}

#[tokio::test]
async fn child_creation_is_scoped_unique_and_transactional() {
    let (_tmp, cfg, app, store) = setup().await;
    let root = store
        .create_run("root", "v1", "fake", "input")
        .await
        .unwrap();
    let lineage = store.lineage(&root.id).await.unwrap();
    assert_eq!(lineage.depth, 0);
    assert_eq!(lineage.root_run_id, root.id);
    assert!(lineage.parent_run_id.is_none());
    assert!(store
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .is_err());
    store
        .request_tool(&root.id, "call", "agent.invoke", &json!({}))
        .await
        .unwrap();
    assert!(store
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .is_err());
    store.start_tool(&root.id, "call").await.unwrap();
    let other = app.agent_runs("other").unwrap();
    assert!(other
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .is_err());
    let child = store
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .unwrap();
    let lineage = store.lineage(&child.id).await.unwrap();
    assert_eq!(lineage.depth, 1);
    assert_eq!(lineage.root_run_id, root.id);
    assert_eq!(lineage.parent_run_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(lineage.parent_call_id.as_deref(), Some("call"));
    let sequence = store
        .get_run(&root.id)
        .await
        .unwrap()
        .unwrap()
        .last_sequence;
    assert!(store
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .is_err());
    assert_eq!(store.history(10).await.unwrap().len(), 2);
    assert_eq!(
        store
            .get_run(&root.id)
            .await
            .unwrap()
            .unwrap()
            .last_sequence,
        sequence
    );
    assert!(other.lineage(&child.id).await.is_err());
    assert!(other.children(&root.id).await.is_err());
    assert!(store
        .append_event(&root.id, "delegation.started", &json!({}))
        .await
        .is_err());
    assert_eq!(
        store.events(&child.id, 0, 10).await.unwrap()[0].event_type,
        "run.created"
    );
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    assert_eq!(store.children(&root.id).await.unwrap()[0].run_id, child.id);
}

#[tokio::test]
async fn wrong_tool_and_depth_limit_cannot_create_children() {
    let (_tmp, _cfg, _app, store) = setup().await;
    let root = store
        .create_run("root", "v1", "fake", "input")
        .await
        .unwrap();
    store
        .request_tool(&root.id, "wrong", "search", &json!({}))
        .await
        .unwrap();
    store.start_tool(&root.id, "wrong").await.unwrap();
    assert!(store
        .create_child_run(&root.id, "wrong", "child", "v1", "fake", "input")
        .await
        .is_err());
    let mut parent = root.id;
    for depth in 1..=4 {
        start(&store, &parent, "call").await;
        let child = store
            .create_child_run(&parent, "call", "child", "v1", "fake", "input")
            .await
            .unwrap();
        assert_eq!(store.lineage(&child.id).await.unwrap().depth, depth);
        parent = child.id;
    }
    start(&store, &parent, "call").await;
    assert!(store
        .create_child_run(&parent, "call", "child", "v1", "fake", "input")
        .await
        .is_err());
    assert_eq!(store.history(10).await.unwrap().len(), 5);
}

#[tokio::test]
async fn terminal_parent_cleans_descendants_and_pending_approvals() {
    for cancelled in [false, true] {
        let (_tmp, _cfg, _app, store) = setup().await;
        let root = store
            .create_run("root", "v1", "fake", "input")
            .await
            .unwrap();
        start(&store, &root.id, "call").await;
        let child = store
            .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
            .await
            .unwrap();
        start(&store, &child.id, "call").await;
        let grandchild = store
            .create_child_run(&child.id, "call", "child", "v1", "fake", "input")
            .await
            .unwrap();
        store
            .request_tool(&grandchild.id, "approval", "process.exec", &json!({}))
            .await
            .unwrap();
        store
            .request_approval(&grandchild.id, "approval", &[Capability::ProcessExecute])
            .await
            .unwrap();
        // A completed invocation alone does not permit abandoning its live child.
        store
            .finish_tool(&root.id, "call", ToolOutcome::Completed(json!({})))
            .await
            .unwrap();
        assert!(store
            .finish_run(&root.id, RunOutcome::Completed("bad".into()))
            .await
            .is_err());
        assert_eq!(
            store.get_run(&root.id).await.unwrap().unwrap().status,
            "running"
        );
        store
            .finish_run(
                &root.id,
                if cancelled {
                    RunOutcome::Cancelled
                } else {
                    RunOutcome::Failed("failed".into())
                },
            )
            .await
            .unwrap();
        for id in [&root.id, &child.id, &grandchild.id] {
            assert_eq!(
                store.get_run(id).await.unwrap().unwrap().status,
                if cancelled { "cancelled" } else { "failed" }
            );
            assert!(!store
                .tool_invocations(id)
                .await
                .unwrap()
                .iter()
                .any(|t| matches!(t.status.as_str(), "requested" | "started")));
        }
        let events = store.events(&grandchild.id, 0, 100).await.unwrap();
        assert!(events.iter().any(|e| e.event_type == "approval.denied"));
        assert!(store
            .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
            .await
            .is_err());
    }
}

#[tokio::test]
async fn legacy_run_is_a_root_and_can_delegate() {
    let (_tmp, cfg, _app, store) = setup().await;
    let root = store
        .create_run("legacy", "v1", "fake", "input")
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", cfg.db.path.display()))
        .await
        .unwrap();
    sqlx::query("DELETE FROM agent_run_lineage WHERE run_id = ?")
        .bind(&root.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(store.lineage(&root.id).await.unwrap().root_run_id, root.id);
    start(&store, &root.id, "call").await;
    let child = store
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .unwrap();
    assert_eq!(store.lineage(&child.id).await.unwrap().root_run_id, root.id);
    assert_eq!(store.lineage(&child.id).await.unwrap().depth, 1);
}

#[tokio::test]
async fn child_and_parent_cannot_reopen_after_delegation() {
    let (_tmp, _cfg, _app, store) = setup().await;
    let root = store
        .create_run("root", "v1", "fake", "input")
        .await
        .unwrap();
    start(&store, &root.id, "call").await;
    let child = store
        .create_child_run(&root.id, "call", "child", "v1", "fake", "input")
        .await
        .unwrap();
    store
        .finish_run(&child.id, RunOutcome::Cancelled)
        .await
        .unwrap();
    store
        .finish_tool(&root.id, "call", ToolOutcome::Completed(json!({})))
        .await
        .unwrap();
    store
        .finish_run(&root.id, RunOutcome::Cancelled)
        .await
        .unwrap();
    for id in [&root.id, &child.id] {
        let sequence = store.get_run(id).await.unwrap().unwrap().last_sequence;
        assert!(store.reopen_run(id, sequence).await.is_err());
        let run = store.get_run(id).await.unwrap().unwrap();
        assert_eq!(run.status, "cancelled");
        assert_eq!(run.last_sequence, sequence);
    }
}

#[tokio::test]
async fn read_only_inspection_supports_database_before_lineage_migration() {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    let (_tmp, cfg, _app, store) = setup().await;
    let root = store
        .create_run("legacy", "v1", "fake", "input")
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", cfg.db.path.display()))
        .await
        .unwrap();
    sqlx::query("DROP TABLE agent_run_lineage")
        .execute(&pool)
        .await
        .unwrap();
    let readonly = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&cfg.db.path)
                .read_only(true)
                .create_if_missing(false)
                .pragma("query_only", "ON"),
        )
        .await
        .unwrap();
    let store = AgentRunStore::new(readonly.clone(), "one").unwrap();
    let lineage = store.lineage(&root.id).await.unwrap();
    assert_eq!(lineage.root_run_id, root.id);
    assert_eq!(lineage.depth, 0);
    assert!(store.children(&root.id).await.unwrap().is_empty());
    let other = AgentRunStore::new(readonly, "other").unwrap();
    assert!(other.lineage(&root.id).await.is_err());
    assert!(other.children(&root.id).await.is_err());
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'agent_run_lineage')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!exists);
}
