use async_trait::async_trait;
use axum::{extract::State, routing::post, Json, Router};
use context_harness::{
    agent_model::{fake::FakeModel, *},
    agent_resource::{AgentResource, LoadedAgentResource, ResourceScope},
    agent_runtime::AgentRuntime,
    agent_store::{RunLifecycle, RunOutcome, RunOutcomeKind, TokenAccounting, ToolOutcome},
    app_store::SqliteAppStore,
    chunk::chunk_text,
    config::Config,
    models::Document,
};
use context_harness_core::store::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
fn budgeted_resource(
    tools: &[&str],
    max_total_tokens: Option<u64>,
    max_tool_calls: Option<u64>,
) -> LoadedAgentResource {
    let mut resource = resource(tools, 4, 10);
    resource.definition.agent.execution.max_total_tokens = max_total_tokens;
    resource.definition.agent.execution.max_tool_calls = max_tool_calls;
    resource.version = resource.definition.version().unwrap();
    resource
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

struct OneResponse(ModelResponse);

#[async_trait]
impl ModelProvider for OneResponse {
    async fn generate(&self, _request: &ModelRequest) -> ModelResult<ModelResponse> {
        Ok(self.0.clone())
    }
}

#[tokio::test]
async fn cumulative_token_budget_rejects_over_budget_final_response() {
    let tmp = TempDir::new().unwrap();
    let mut response = ModelResponse::text("must not be accepted");
    response.usage = Some(Usage {
        input_tokens: 7,
        output_tokens: 4,
        total_tokens: 11,
    });
    let runtime = AgentRuntime::new(
        config(tmp.path()),
        tmp.path(),
        registry(Arc::new(OneResponse(response))),
    )
    .await
    .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&budgeted_resource(&[], Some(10), None), "question", cancel)
        .await
        .unwrap();

    assert_eq!(run.outcome, Some(RunOutcomeKind::LimitExceeded));
    assert_eq!(run.reason_code.as_deref(), Some("total_tokens"));
    assert_eq!(run.output, None);
    assert_eq!(run.usage.model_turns, 1);
    assert_eq!(run.usage.total_tokens, Some(11));
    assert_eq!(run.usage.token_accounting, TokenAccounting::Complete);
    let stopped = runtime
        .store()
        .events(&run.id, 0, 20)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(stopped.event_type, "run.limit_exceeded");
    assert_eq!(stopped.payload["usage"]["total_tokens"], 11);
    assert_eq!(stopped.payload["budgets"]["max_total_tokens"], 10);
}

#[tokio::test]
async fn configured_token_budget_fails_closed_when_usage_is_missing() {
    let tmp = TempDir::new().unwrap();
    let runtime = AgentRuntime::new(
        config(tmp.path()),
        tmp.path(),
        registry(Arc::new(OneResponse(ModelResponse::text("not accepted")))),
    )
    .await
    .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&budgeted_resource(&[], Some(10), None), "question", cancel)
        .await
        .unwrap();

    assert_eq!(run.outcome, Some(RunOutcomeKind::LimitExceeded));
    assert_eq!(run.reason_code.as_deref(), Some("token_usage_unavailable"));
    assert_eq!(run.usage.responses_without_usage, 1);
    assert_eq!(run.usage.total_tokens, None);
    assert_eq!(run.usage.token_accounting, TokenAccounting::Unavailable);
}

#[tokio::test]
async fn missing_usage_without_token_budget_remains_observable_but_completes() {
    let tmp = TempDir::new().unwrap();
    let runtime = AgentRuntime::new(
        config(tmp.path()),
        tmp.path(),
        registry(Arc::new(OneResponse(ModelResponse::text("accepted")))),
    )
    .await
    .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&budgeted_resource(&[], None, None), "question", cancel)
        .await
        .unwrap();

    assert_eq!(run.outcome, Some(RunOutcomeKind::Completed));
    assert_eq!(run.output.as_deref(), Some("accepted"));
    assert_eq!(run.usage.total_tokens, None);
    assert_eq!(run.usage.token_accounting, TokenAccounting::Unavailable);
}

#[tokio::test]
async fn cumulative_tool_budget_rejects_a_whole_response_batch() {
    let tmp = TempDir::new().unwrap();
    let mut response = call("search", "one", json!({"query":"first"}));
    response.tool_calls.push(ToolCall {
        name: "get".into(),
        id: "two".into(),
        arguments: json!({"id":"doc"}),
    });
    let runtime = AgentRuntime::new(
        config(tmp.path()),
        tmp.path(),
        registry(Arc::new(OneResponse(response))),
    )
    .await
    .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(
            &budgeted_resource(&["search", "get"], None, Some(1)),
            "question",
            cancel,
        )
        .await
        .unwrap();

    assert_eq!(run.outcome, Some(RunOutcomeKind::LimitExceeded));
    assert_eq!(run.reason_code.as_deref(), Some("tool_calls"));
    assert_eq!(run.usage.tool_calls, 0);
    assert!(runtime
        .store()
        .tool_invocations(&run.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn opt_in_run_controls_suspend_without_tool_execution() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(tmp.path());
    let response = call(
        "run.blocked",
        "blocked-1",
        json!({"code":"environment_unavailable","message":"fixture dependency is offline"}),
    );
    let runtime = AgentRuntime::new(cfg, tmp.path(), registry(Arc::new(OneResponse(response))))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(
            &resource(&["run.blocked"], 2, 10),
            "Wait for the fixture",
            cancel,
        )
        .await
        .unwrap();

    assert_eq!(run.status, "failed");
    assert_eq!(run.lifecycle, RunLifecycle::Suspended);
    assert_eq!(run.outcome, Some(RunOutcomeKind::Blocked));
    assert_eq!(run.reason_code.as_deref(), Some("environment_unavailable"));
    assert_eq!(
        run.reason_detail.unwrap().0["message"],
        "fixture dependency is offline"
    );
    assert!(runtime
        .store()
        .tool_invocations(&run.id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        runtime
            .store()
            .events(&run.id, 0, 20)
            .await
            .unwrap()
            .last()
            .unwrap()
            .event_type,
        "run.blocked"
    );
}

#[tokio::test]
async fn request_user_input_suspends_with_a_typed_question() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(tmp.path());
    let response = call(
        "run.request_user_input",
        "question-1",
        json!({"code":"information_required","question":"Which release should I inspect?"}),
    );
    let runtime = AgentRuntime::new(cfg, tmp.path(), registry(Arc::new(OneResponse(response))))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(
            &resource(&["run.request_user_input"], 2, 10),
            "Inspect a release",
            cancel,
        )
        .await
        .unwrap();

    assert_eq!(run.lifecycle, RunLifecycle::Suspended);
    assert_eq!(run.outcome, Some(RunOutcomeKind::NeedsUserInput));
    assert_eq!(run.reason_code.as_deref(), Some("information_required"));
    assert_eq!(
        run.reason_detail.unwrap().0["question"],
        "Which release should I inspect?"
    );
    assert!(runtime
        .store()
        .tool_invocations(&run.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn malformed_run_control_is_a_typed_failure_without_dispatch() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(tmp.path());
    let response = call(
        "run.blocked",
        "blocked-1",
        json!({"code":"environment_unavailable","message":"offline","extra":true}),
    );
    let runtime = AgentRuntime::new(cfg, tmp.path(), registry(Arc::new(OneResponse(response))))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&resource(&["run.blocked"], 2, 10), "Wait", cancel)
        .await
        .unwrap();

    assert_eq!(run.lifecycle, RunLifecycle::Terminal);
    assert_eq!(run.outcome, Some(RunOutcomeKind::Failed));
    assert_eq!(run.reason_code.as_deref(), Some("invalid_model_response"));
    assert!(runtime
        .store()
        .tool_invocations(&run.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn mixed_run_control_and_ordinary_tool_fails_before_dispatch() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(tmp.path());
    let mut response = call(
        "run.request_user_input",
        "question-1",
        json!({"code":"decision_required","question":"Choose A or B"}),
    );
    response.tool_calls.push(ToolCall {
        name: "search".into(),
        id: "search-1".into(),
        arguments: json!({"query":"must not run"}),
    });
    let runtime = AgentRuntime::new(cfg, tmp.path(), registry(Arc::new(OneResponse(response))))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(
            &resource(&["run.request_user_input", "search"], 2, 10),
            "Ask if needed",
            cancel,
        )
        .await
        .unwrap();

    assert_eq!(run.outcome, Some(RunOutcomeKind::Failed));
    assert_eq!(run.reason_code.as_deref(), Some("invalid_model_response"));
    assert!(runtime
        .store()
        .tool_invocations(&run.id)
        .await
        .unwrap()
        .is_empty());
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
    let resolved = events
        .iter()
        .find(|event| event.event_type == "context.resolved")
        .unwrap();
    assert_eq!(
        resolved.payload["tool_bindings"]["search"]["implementation_id"],
        "builtin.compat.search"
    );
    assert_eq!(
        resolved.payload["tool_bindings"]["get"]["compatibility"],
        true
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
        if turns == 1 {
            assert_eq!(run.outcome, Some(RunOutcomeKind::LimitExceeded));
            assert_eq!(run.reason_code.as_deref(), Some("model_turns"));
        } else {
            assert_eq!(run.outcome, Some(RunOutcomeKind::Failed));
            assert_eq!(run.reason_code.as_deref(), Some("model_error"));
        }
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
            assert_eq!(run.outcome, Some(RunOutcomeKind::LimitExceeded));
            assert_eq!(run.reason_code.as_deref(), Some("duration"));
        }
        let expected_event = if cancel_early {
            "run.cancelled"
        } else {
            "run.limit_exceeded"
        };
        assert_eq!(
            runtime
                .store()
                .events(&run.id, 0, 100)
                .await
                .unwrap()
                .last()
                .unwrap()
                .event_type,
            expected_event
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

#[tokio::test]
async fn ollama_agent_completes_search_get_and_final_answer() {
    #[derive(Clone)]
    struct OllamaFixture {
        calls: Arc<AtomicUsize>,
    }
    async fn chat(State(state): State<OllamaFixture>, Json(body): Json<Value>) -> Json<Value> {
        let wire = |name: &str| format!("{:x}", Sha256::digest(name.as_bytes()));
        let (content, tool_calls) = match state.calls.fetch_add(1, Ordering::SeqCst) {
            0 => (
                "",
                json!([{"type":"function","function":{
                    "name":wire("search"), "arguments":{"query":"deployment"}
                }}]),
            ),
            1 => {
                assert_eq!(
                    body["messages"].as_array().unwrap().last().unwrap()["role"],
                    "tool"
                );
                (
                    "",
                    json!([{"type":"function","function":{
                        "name":wire("get"), "arguments":{"id":"doc-a"}
                    }}]),
                )
            }
            2 => {
                assert!(
                    body["messages"].as_array().unwrap().last().unwrap()["content"]
                        .as_str()
                        .unwrap()
                        .contains("local SQLite")
                );
                ("The deployment uses local SQLite.", json!([]))
            }
            _ => panic!("unexpected extra Ollama call"),
        };
        Json(json!({
            "model":"qwen3", "done":true, "done_reason":"stop",
            "message":{"role":"assistant", "content":content, "tool_calls":tool_calls},
            "prompt_eval_count":10, "eval_count":2
        }))
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/api/chat", post(chat))
        .with_state(OllamaFixture {
            calls: calls.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let tmp = TempDir::new().unwrap();
    let mut cfg = config(tmp.path());
    cfg.models.insert(
        "test".into(),
        context_harness::agent_resource::ModelDefinition {
            provider: "ollama".into(),
            model: "qwen3".into(),
            base_url: Some(base_url),
            timeout_seconds: Some(10),
            ..Default::default()
        },
    );
    seed(&cfg, "Our deployment uses local SQLite.").await;
    let models = ModelRegistry::from_config(&cfg.models).unwrap();
    let runtime = AgentRuntime::new(cfg, tmp.path(), models).await.unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(
            &resource(&["search", "get"], 4, 10),
            "Explain deployment",
            cancel,
        )
        .await
        .unwrap();
    server.abort();

    assert_eq!(run.status, "completed");
    assert_eq!(
        run.output.as_deref(),
        Some("The deployment uses local SQLite.")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let tool_calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(tool_calls.len(), 2);
    assert!(tool_calls.iter().all(|call| call.status == "completed"));
}
