use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, *},
    agent_resource::{AgentResource, LoadedAgentResource, ResourceScope},
    agent_runtime::AgentRuntime,
    agent_store::{RunOutcome, ToolOutcome},
    app_store::SqliteAppStore,
    chunk::chunk_text,
    config::Config,
    models::Document,
};
use context_harness_core::store::Store;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::watch;

fn config(root: &Path) -> Config {
    let mut config = Config::minimal();
    config.db.path = root.join("ctx.sqlite");
    config
}
fn resource(tools: &[&str], max_turns: u32, timeout: u64) -> LoadedAgentResource {
    let source = format!("[agent]\nname='researcher'\nmodel='test'\ntools={}\n[agent.execution]\nmax_turns={max_turns}\ntimeout_seconds={timeout}\n[prompt]\nsystem='Search then retrieve project context.'", serde_json::to_string(tools).unwrap());
    let definition = AgentResource::parse(&source).unwrap();
    LoadedAgentResource {
        path: "test.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    }
}
fn registry(provider: Arc<dyn ModelProvider>) -> ModelRegistry {
    let mut models = ModelRegistry::default();
    models
        .register("test", "fake", "scripted", provider)
        .unwrap();
    models
}
fn call(name: &str, id: &str, args: Value) -> ModelResponse {
    let mut response = ModelResponse::text("");
    response.finish_reason = FinishReason::ToolCalls;
    response.tool_calls = vec![ToolCall {
        name: name.into(),
        id: id.into(),
        arguments: args,
    }];
    response
}
async fn seed(cfg: &Config, text: &str) {
    SqliteAppStore::initialize_config(cfg).await.unwrap();
    let app = SqliteAppStore::connect(cfg).await.unwrap();
    let doc = Document {
        id: "doc-a".into(),
        source: "filesystem:test".into(),
        source_id: "a.md".into(),
        source_url: None,
        title: Some("Guide".into()),
        author: None,
        created_at: 1,
        updated_at: 2,
        content_type: "text/plain".into(),
        body: text.into(),
        metadata_json: "{}".into(),
        raw_json: None,
        dedup_hash: "fixture".into(),
    };
    let id = app.upsert_document(&doc).await.unwrap();
    app.replace_chunks(&id, &chunk_text(&id, text, 700), None)
        .await
        .unwrap();
    app.close().await;
}
struct Grounded {
    calls: AtomicUsize,
}
#[async_trait]
impl ModelProvider for Grounded {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(call("search", "s1", json!({"query":"deployment"}))),
            1 => {
                let ModelMessage::Tool { content, .. } = request.messages.last().unwrap() else {
                    panic!("missing search result")
                };
                let result: Value = serde_json::from_str(content).unwrap();
                assert_eq!(result["results"].as_array().unwrap().len(), 1);
                Ok(call("get", "g1", json!({"id":result["results"][0]["id"]})))
            }
            2 => {
                let ModelMessage::Tool { content, .. } = request.messages.last().unwrap() else {
                    panic!("missing document")
                };
                assert!(content.contains("deployment uses local SQLite"));
                Ok(ModelResponse::text("The deployment uses local SQLite."))
            }
            _ => panic!("unexpected extra turn"),
        }
    }
}

#[tokio::test]
async fn executes_search_get_and_final_answer_with_durable_tool_history() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(tmp.path());
    seed(&cfg, "Our deployment uses local SQLite.").await;
    let provider = Arc::new(Grounded {
        calls: AtomicUsize::new(0),
    });
    let runtime = AgentRuntime::new(cfg.clone(), tmp.path(), registry(provider.clone()))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(
            &resource(&["search", "get"], 4, 10),
            "Explain deployment",
            cancel,
        )
        .await
        .unwrap();
    assert_eq!(run.status, "completed");
    assert_eq!(
        run.output.as_deref(),
        Some("The deployment uses local SQLite.")
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| call.status == "completed"));
    assert!(calls[1]
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("local SQLite"));
    let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "tool.started")
            .count(),
        2
    );
    assert_eq!(events.last().unwrap().event_type, "run.completed");
    let app = SqliteAppStore::connect(&cfg).await.unwrap();
    let stored = app.agent_runs(&run.workspace_id).unwrap();
    assert_eq!(
        stored.get_run(&run.id).await.unwrap().unwrap().output,
        run.output
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents")
        .fetch_one(app.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn malformed_or_network_search_arguments_are_denied_before_tool_start() {
    for args in [
        json!({"query":"x", "mode":"semantic"}),
        json!({"query":"x", "limit":-1}),
        json!({"query":"x", "workspace":"other"}),
    ] {
        let tmp = TempDir::new().unwrap();
        let provider = Arc::new(FakeModel::new([Ok(call("search", "s1", args))]));
        let runtime = AgentRuntime::new(config(tmp.path()), tmp.path(), registry(provider))
            .await
            .unwrap();
        let (_sender, cancel) = watch::channel(false);
        let run = runtime
            .run(&resource(&["search"], 2, 10), "query", cancel)
            .await
            .unwrap();
        assert_eq!(run.status, "failed");
        let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
        assert_eq!(calls[0].status, "denied");
        assert!(calls[0].started_at.is_none());
    }
}

#[tokio::test]
async fn turn_limit_stops_before_tools_and_unknown_tool_calls_never_dispatch() {
    for (declared, name, turns) in [
        (vec!["search"], "search", 1),
        (vec!["get"], "workspace.patch", 3),
    ] {
        let tmp = TempDir::new().unwrap();
        let runtime = AgentRuntime::new(
            config(tmp.path()),
            tmp.path(),
            registry(Arc::new(FakeModel::new([Ok(call(
                name,
                "c1",
                json!({"query":"x"}),
            ))]))),
        )
        .await
        .unwrap();
        let (_sender, cancel) = watch::channel(false);
        let run = runtime
            .run(&resource(&declared, turns, 10), "query", cancel)
            .await
            .unwrap();
        assert_eq!(run.status, "failed");
        assert!(runtime
            .store()
            .tool_invocations(&run.id)
            .await
            .unwrap()
            .is_empty());
    }
}

struct Slow {
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl ModelProvider for Slow {
    async fn generate(&self, _request: &ModelRequest) -> ModelResult<ModelResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(ModelResponse::text("late"))
    }
}
#[tokio::test]
async fn timeout_and_explicit_cancellation_persist_terminal_runs() {
    for cancel_early in [false, true] {
        let tmp = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = AgentRuntime::new(
            config(tmp.path()),
            tmp.path(),
            registry(Arc::new(Slow {
                calls: calls.clone(),
            })),
        )
        .await
        .unwrap();
        let (sender, receiver) = watch::channel(false);
        let cancel_task = tokio::spawn(async move {
            if cancel_early {
                tokio::time::sleep(Duration::from_millis(50)).await;
                sender.send(true).unwrap();
            }
        });
        let run = runtime
            .run(&resource(&[], 2, 1), "question", receiver)
            .await
            .unwrap();
        cancel_task.await.unwrap();
        assert_eq!(
            run.status,
            if cancel_early { "cancelled" } else { "failed" }
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if !cancel_early {
            assert_eq!(run.error.as_deref(), Some("execution timeout"));
        }
        assert_eq!(
            runtime
                .store()
                .events(&run.id, 0, 100)
                .await
                .unwrap()
                .last()
                .unwrap()
                .event_type,
            format!("run.{}", run.status)
        );
    }
}

#[tokio::test]
async fn unsupported_declarations_and_approval_requirements_fail_before_model_calls() {
    for denied_policy in [false, true] {
        let tmp = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = AgentRuntime::new(
            config(tmp.path()),
            tmp.path(),
            registry(Arc::new(Slow {
                calls: calls.clone(),
            })),
        )
        .await
        .unwrap();
        let mut resource = resource(
            if denied_policy {
                &["search"]
            } else {
                &["process.exec"]
            },
            2,
            10,
        );
        if denied_policy {
            resource.definition.agent.permissions.allow = Some(vec![]);
        }
        let (_sender, cancel) = watch::channel(false);
        let run = runtime.run(&resource, "question", cancel).await.unwrap();
        assert_eq!(run.status, "failed");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn tool_failures_are_fatal_and_pending_invocations_are_cleaned_up_atomically() {
    let tmp = TempDir::new().unwrap();
    let runtime = AgentRuntime::new(
        config(tmp.path()),
        tmp.path(),
        registry(Arc::new(FakeModel::new([Ok(call(
            "get",
            "g1",
            json!({"id":"private-missing-id"}),
        ))]))),
    )
    .await
    .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&resource(&["get"], 2, 10), "question", cancel)
        .await
        .unwrap();
    assert_eq!(run.status, "failed");
    assert!(!run.error.unwrap().contains("private-missing-id"));
    assert_eq!(
        runtime.store().tool_invocations(&run.id).await.unwrap()[0].status,
        "failed"
    );
    let store = runtime.store();
    let pending = store
        .create_run("test", "v1", "test", "task")
        .await
        .unwrap();
    store
        .request_tool(&pending.id, "pending", "search", &json!({}))
        .await
        .unwrap();
    store.start_tool(&pending.id, "pending").await.unwrap();
    assert!(store
        .finish_tool(
            &pending.id,
            "pending",
            ToolOutcome::Denied("bad transition".into())
        )
        .await
        .is_err());
    assert!(store
        .finish_run(&pending.id, RunOutcome::Completed("premature".into()))
        .await
        .is_err());
    store
        .finish_run(&pending.id, RunOutcome::Cancelled)
        .await
        .unwrap();
    let events = store.events(&pending.id, 0, 100).await.unwrap();
    assert_eq!(events[events.len() - 2].event_type, "tool.failed");
    assert_eq!(events.last().unwrap().event_type, "run.cancelled");
    assert_eq!(
        store.tool_invocations(&pending.id).await.unwrap()[0].status,
        "failed"
    );
}

#[tokio::test]
async fn shared_database_history_is_isolated_by_canonical_workspace_root() {
    let tmp = TempDir::new().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let cfg = config(tmp.path());
    let ra = AgentRuntime::new(
        cfg.clone(),
        &a,
        registry(Arc::new(FakeModel::new([Ok(ModelResponse::text("A"))]))),
    )
    .await
    .unwrap();
    let rb = AgentRuntime::new(cfg, &b, ModelRegistry::default())
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = ra
        .run(&resource(&[], 1, 10), "question", cancel)
        .await
        .unwrap();
    assert!(rb.store().get_run(&run.id).await.unwrap().is_none());
    assert!(rb.store().history(10).await.unwrap().is_empty());
    assert!(rb
        .store()
        .request_tool(&run.id, "foreign", "get", &json!({}))
        .await
        .is_err());
    assert!(rb.store().tool_invocations(&run.id).await.is_err());
}
