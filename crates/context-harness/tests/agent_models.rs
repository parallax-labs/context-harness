use context_harness::agent_model::{fake::FakeModel, *};
use context_harness::agent_resource::ModelDefinition;
use context_harness::agent_store::RunOutcome;
use context_harness::app_store::SqliteAppStore;
use context_harness::config::Config;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::TempDir;

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![ModelMessage::User {
            content: "sensitive question".into(),
        }],
        ..Default::default()
    }
}
fn config(tmp: &TempDir) -> Config {
    toml::from_str(&format!("[db]\npath = {:?}\n[chunking]\nmax_tokens = 700\n[retrieval]\nfinal_limit = 12\n[server]\nbind = '127.0.0.1:0'\n", tmp.path().join("ctx.sqlite").to_str().unwrap())).unwrap()
}

#[tokio::test]
async fn scripted_model_can_drive_multiple_tool_turns_without_network() {
    let call = ToolCall {
        id: "call-1".into(),
        name: "search".into(),
        arguments: json!({"query":"design"}),
    };
    let mut tool_response = ModelResponse::text("");
    tool_response.tool_calls.push(call);
    tool_response.finish_reason = FinishReason::ToolCalls;
    let mut registry = ModelRegistry::default();
    registry
        .register(
            "reasoning",
            "fake",
            "test",
            Arc::new(FakeModel::new([
                Ok(tool_response),
                Ok(ModelResponse::text("Grounded answer")),
            ])),
        )
        .unwrap();
    let mut req = request();
    req.tools.push(ModelTool {
        name: "search".into(),
        description: "Search context".into(),
        parameters: json!({"type":"object"}),
    });
    let first = registry.generate("reasoning", &req).await.unwrap();
    assert_eq!(first.tool_calls[0].arguments["query"], "design");
    req.messages.push(first.message());
    // Outstanding calls are rejected before consuming the next fake outcome.
    assert!(registry.generate("reasoning", &req).await.is_err());
    req.messages.push(ModelMessage::Tool {
        call_id: "call-1".into(),
        content: "retrieved source".into(),
    });
    let final_response = registry.generate("reasoning", &req).await.unwrap();
    assert_eq!(final_response.text, "Grounded answer");
    assert_eq!(final_response.finish_reason, FinishReason::Completed);
    let exhausted = registry.generate("reasoning", &req).await.unwrap_err();
    assert_eq!(
        exhausted.downcast_ref::<ModelError>().unwrap().kind,
        ModelErrorKind::ScriptExhausted
    );
}

#[tokio::test]
async fn lifecycle_events_persist_metadata_only_and_keep_workspace_binding() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let app = SqliteAppStore::connect(&cfg).await.unwrap();
    let store = app.agent_runs("workspace").unwrap();
    let other = app.agent_runs("other").unwrap();
    let run = store
        .create_run("agent", "v1", "reasoning", "task")
        .await
        .unwrap();
    let mut registry = ModelRegistry::default();
    let mut response = ModelResponse::text("sensitive answer");
    response.usage = Some(Usage {
        input_tokens: 4,
        output_tokens: 2,
        total_tokens: 6,
    });
    registry
        .register(
            "reasoning",
            "fake",
            "scripted",
            Arc::new(FakeModel::new([Ok(response)])),
        )
        .unwrap();
    assert!(registry
        .generate_recorded(&other, &run.id, "reasoning", &request())
        .await
        .is_err());
    let response = registry
        .generate_recorded(&store, &run.id, "reasoning", &request())
        .await
        .unwrap();
    assert_eq!(response.text, "sensitive answer");
    app.close().await;
    let reopened = SqliteAppStore::connect(&cfg).await.unwrap();
    let events = reopened
        .agent_runs("workspace")
        .unwrap()
        .events(&run.id, 0, 10)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        ["run.started", "model.requested", "model.responded"]
    );
    assert_eq!(events[1].payload["call_id"], events[2].payload["call_id"]);
    assert_eq!(events[2].payload["usage"]["total_tokens"], 6);
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains("sensitive"));
    assert!(!serialized.contains("api_key"));
}

#[tokio::test]
async fn model_failure_is_recorded_but_does_not_finish_the_run() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let app = SqliteAppStore::connect(&cfg).await.unwrap();
    let store = app.agent_runs("workspace").unwrap();
    let run = store
        .create_run("agent", "v1", "reasoning", "task")
        .await
        .unwrap();
    let mut registry = ModelRegistry::default();
    registry
        .register(
            "reasoning",
            "fake",
            "scripted",
            Arc::new(FakeModel::new([Err(ModelError {
                kind: ModelErrorKind::RateLimited,
                http_status: Some(429),
            })])),
        )
        .unwrap();
    assert!(registry
        .generate_recorded(&store, &run.id, "reasoning", &request())
        .await
        .is_err());
    let events = store.events(&run.id, 0, 10).await.unwrap();
    assert_eq!(events[2].event_type, "model.failed");
    assert_eq!(events[2].payload["error"]["kind"], "rate_limited");
    assert_eq!(
        store.get_run(&run.id).await.unwrap().unwrap().status,
        "running"
    );
}

#[tokio::test]
async fn alias_mismatch_terminal_runs_and_failed_event_writes_do_not_call_model() {
    let tmp = TempDir::new().unwrap();
    let cfg = config(&tmp);
    SqliteAppStore::initialize_config(&cfg).await.unwrap();
    let app = SqliteAppStore::connect(&cfg).await.unwrap();
    let store = app.agent_runs("workspace").unwrap();
    let run = store
        .create_run("agent", "v1", "reasoning", "task")
        .await
        .unwrap();
    let mut registry = ModelRegistry::default();
    registry
        .register(
            "reasoning",
            "fake",
            "test",
            Arc::new(FakeModel::new([Ok(ModelResponse::text("unconsumed"))])),
        )
        .unwrap();
    registry
        .register("other", "fake", "test", Arc::new(FakeModel::new([])))
        .unwrap();
    assert!(registry
        .generate_recorded(&store, &run.id, "other", &request())
        .await
        .is_err());
    sqlx::query("CREATE TRIGGER reject_model_event BEFORE INSERT ON agent_events WHEN NEW.event_type = 'model.requested' BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
        .execute(app.pool()).await.unwrap();
    assert!(registry
        .generate_recorded(&store, &run.id, "reasoning", &request())
        .await
        .is_err());
    sqlx::query("DROP TRIGGER reject_model_event")
        .execute(app.pool())
        .await
        .unwrap();
    store
        .finish_run(&run.id, RunOutcome::Cancelled)
        .await
        .unwrap();
    assert!(registry
        .generate_recorded(&store, &run.id, "reasoning", &request())
        .await
        .is_err());
    assert_eq!(
        registry
            .generate("reasoning", &request())
            .await
            .unwrap()
            .text,
        "unconsumed"
    );
}

#[tokio::test]
async fn config_registry_is_lazy_about_credentials_and_rejects_unknown_providers() {
    let mut models = BTreeMap::new();
    models.insert(
        "real".into(),
        ModelDefinition {
            provider: "openai".into(),
            model: "test-model".into(),
            api_key_env: Some(format!("CTX_ABSENT_{}", uuid::Uuid::new_v4().simple())),
        },
    );
    models.insert(
        "fake".into(),
        ModelDefinition {
            provider: "fake".into(),
            model: "test-model".into(),
            api_key_env: None,
        },
    );
    let registry = ModelRegistry::from_config(&models).unwrap();
    let response = registry.generate("fake", &request()).await.unwrap();
    assert!(response.text.contains("Synthetic response"));
    let error = registry.generate("real", &request()).await.unwrap_err();
    assert_eq!(
        error.downcast_ref::<ModelError>().unwrap().kind,
        ModelErrorKind::MissingCredentials
    );
    models.get_mut("real").unwrap().provider = "unknown".into();
    assert!(ModelRegistry::from_config(&models).is_err());
}

#[test]
fn rejects_unmatched_results_duplicate_ids_and_bad_tool_schemas() {
    let mut req = request();
    req.messages.push(ModelMessage::Tool {
        call_id: "unknown".into(),
        content: "bad".into(),
    });
    assert!(req.validate().is_err());
    let mut req = request();
    req.tools.push(ModelTool {
        name: "search".into(),
        description: String::new(),
        parameters: json!(null),
    });
    assert!(req.validate().is_err());
    let mut req = request();
    let call = ToolCall {
        id: "same".into(),
        name: "search".into(),
        arguments: json!({}),
    };
    req.messages.push(ModelMessage::Assistant {
        text: String::new(),
        tool_calls: vec![call.clone(), call],
        continuation: None,
    });
    assert!(req.validate().is_err());
}
