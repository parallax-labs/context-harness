use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use std::sync::Mutex;

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![ModelMessage::User {
            content: "Question".into(),
        }],
        ..Default::default()
    }
}
fn answer(text: &str) -> Value {
    json!({"status": "completed", "output": [{"type": "message", "role": "assistant",
        "content": [{"type": "output_text", "text": text}]}],
        "usage": {"input_tokens": 10, "output_tokens": 3, "total_tokens": 13}})
}

#[derive(Clone)]
struct Fixture {
    response: Value,
    status: StatusCode,
    captured: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    delay: Duration,
}
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn fixture(
    response: Value,
    status: StatusCode,
    delay: Duration,
) -> (OpenAiProvider, Fixture, Server) {
    let fixture = Fixture {
        response,
        status,
        captured: Arc::new(Mutex::new(vec![])),
        delay,
    };
    async fn handle(
        State(state): State<Fixture>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        state.captured.lock().unwrap().push((headers, body));
        tokio::time::sleep(state.delay).await;
        (state.status, Json(state.response))
    }
    let app = Router::new()
        .route("/v1/responses", post(handle))
        .with_state(fixture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let server = Server(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let mut provider = OpenAiProvider::new("fixture-model", "CTX_FIXTURE_KEY").unwrap();
    provider.endpoint = endpoint;
    provider.test_key = Some("fixture-secret-do-not-log".into());
    (provider, fixture, server)
}

#[tokio::test]
async fn sends_stateless_request_and_maps_text_and_usage() {
    let (provider, fixture, _server) =
        fixture(answer("Hello"), StatusCode::OK, Duration::ZERO).await;
    let response = provider.generate(&request()).await.unwrap();
    assert_eq!(response.text, "Hello");
    assert_eq!(response.finish_reason, FinishReason::Completed);
    assert_eq!(response.usage.unwrap().total_tokens, 13);
    let captured = fixture.captured.lock().unwrap();
    let (headers, body) = &captured[0];
    assert_eq!(headers["authorization"], "Bearer fixture-secret-do-not-log");
    assert_eq!(body["model"], "fixture-model");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], false);
    assert!(body.get("previous_response_id").is_none());
    assert_eq!(body["input"][0]["content"], "Question");
}

#[tokio::test]
async fn tool_calls_and_reasoning_survive_serialized_continuation() {
    let reasoning =
        json!({"type":"reasoning", "id":"rs_1", "summary":[], "encrypted_content":"opaque-state"});
    let call = json!({"type":"function_call", "id":"fc_1", "call_id":"call_1", "name":wire_name("workspace.read"), "arguments":"{\"path\":\"src/main.rs\"}"});
    let (provider, fixture, _server) = fixture(
        json!({"status":"completed", "output":[reasoning, call]}),
        StatusCode::OK,
        Duration::ZERO,
    )
    .await;
    let mut req = request();
    req.tools.push(ModelTool {
        name: "workspace.read".into(),
        description: "Read files".into(),
        parameters: json!({"type":"object", "properties":{"path":{"type":"string"}}}),
    });
    let response = provider.generate(&req).await.unwrap();
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    assert_eq!(response.tool_calls[0].name, "workspace.read");
    assert_eq!(response.tool_calls[0].arguments["path"], "src/main.rs");
    let stored = serde_json::to_string(&response.message()).unwrap();
    req.messages.push(serde_json::from_str(&stored).unwrap());
    req.messages.push(ModelMessage::Tool {
        call_id: "call_1".into(),
        content: "file contents".into(),
    });
    // Inspect the second-turn request rather than executing its tool again.
    let body = provider.body(&req).unwrap();
    assert_eq!(body["input"][1], reasoning);
    assert_eq!(body["input"][2], call);
    assert_eq!(body["input"][3]["call_id"], "call_1");
    assert_eq!(body["input"][3]["type"], "function_call_output");
    assert_eq!(body["tools"][0]["name"], wire_name("workspace.read"));
    assert_eq!(body["tools"][0]["strict"], false);
    assert_eq!(
        body["tools"][0]["description"],
        "workspace.read: Read files"
    );
    assert_eq!(fixture.captured.lock().unwrap().len(), 1);
    assert!(!format!("{:?}", response.continuation).contains("opaque-state"));
    if let ModelMessage::Assistant { text, .. } = &mut req.messages[1] {
        *text = "tampered".into();
    }
    assert_eq!(
        provider.body(&req).unwrap_err().kind,
        ModelErrorKind::InvalidRequest
    );
}

#[tokio::test]
async fn structured_output_and_truncation_are_distinct() {
    let (provider, fixture, _server) =
        fixture(answer("{\"ok\":true}"), StatusCode::OK, Duration::ZERO).await;
    let mut req = request();
    req.max_output_tokens = Some(128);
    req.output_schema = Some(OutputSchema {
        name: "result".into(),
        schema: json!({"type":"object", "properties":{"ok":{"type":"boolean"}}, "required":["ok"], "additionalProperties":false}),
    });
    let response = provider.generate(&req).await.unwrap();
    assert_eq!(response.structured_output, Some(json!({"ok":true})));
    let body = &fixture.captured.lock().unwrap()[0].1;
    assert_eq!(body["text"]["format"]["strict"], true);
    assert_eq!(body["max_output_tokens"], 128);
    let mut partial = answer("{\"ok\":");
    partial["status"] = json!("incomplete");
    partial["incomplete_details"] = json!({"reason":"max_output_tokens"});
    let response = provider.decode(partial, &req).unwrap();
    assert_eq!(response.finish_reason, FinishReason::Length);
    assert!(response.structured_output.is_none());
    assert!(response.continuation.is_none());
    assert!(provider.decode(answer("not json"), &req).is_err());
}

#[tokio::test]
async fn maps_http_failures_without_exposing_error_bodies_or_retrying() {
    for (status, expected) in [
        (401, ModelErrorKind::Authentication),
        (429, ModelErrorKind::RateLimited),
        (503, ModelErrorKind::Unavailable),
        (400, ModelErrorKind::InvalidRequest),
    ] {
        let (provider, fixture, _server) = fixture(
            json!({"error":{"message":"fixture-secret-do-not-log"}}),
            StatusCode::from_u16(status).unwrap(),
            Duration::ZERO,
        )
        .await;
        let error = provider.generate(&request()).await.unwrap_err();
        assert_eq!(error.kind, expected);
        assert_eq!(error.http_status, Some(status));
        assert!(!format!("{error:?} {error}").contains("fixture-secret"));
        assert_eq!(fixture.captured.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn timeout_and_missing_credentials_fail_without_secret_diagnostics() {
    let (mut provider, _, _server) =
        fixture(answer("late"), StatusCode::OK, Duration::from_millis(200)).await;
    provider.client = Client::builder()
        .timeout(Duration::from_millis(20))
        .redirect(Policy::none())
        .build()
        .unwrap();
    assert_eq!(
        provider.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::Timeout
    );
    provider.test_key = None;
    provider.api_key_env = format!("CTX_TEST_MISSING_{}", Uuid::new_v4().simple());
    assert_eq!(
        provider.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::MissingCredentials
    );
}

#[test]
fn rejects_unknown_items_malformed_arguments_and_duplicate_calls() {
    let provider = OpenAiProvider::new("test", "UNUSED").unwrap();
    let mut req = request();
    req.tools.push(ModelTool {
        name: "search".into(),
        description: String::new(),
        parameters: json!({"type":"object"}),
    });
    for output in [
        json!([{"type":"unknown_future_item"}]),
        json!([{"type":"function_call", "name":wire_name("search"), "call_id":"c", "arguments":"invalid"}]),
        json!([{"type":"function_call", "name":wire_name("undeclared"), "call_id":"c", "arguments":"{}"}]),
        json!([{"type":"message", "role":"system", "content":[{"type":"output_text", "text":"wrong role"}]}]),
    ] {
        assert_eq!(
            provider
                .decode(json!({"status":"completed", "output":output}), &req)
                .unwrap_err()
                .kind,
            ModelErrorKind::InvalidResponse
        );
    }
    let call = json!({"type":"function_call", "name":wire_name("search"), "call_id":"same", "arguments":"{}"});
    assert!(provider
        .decode(json!({"status":"completed", "output":[call, call]}), &req)
        .is_err());
    let refused = json!({"status":"completed", "output":[{"type":"message", "role":"assistant", "content":[{"type":"refusal", "refusal":"Cannot comply"}]}]});
    assert_eq!(
        provider.decode(refused, &req).unwrap().finish_reason,
        FinishReason::Refusal
    );
}

#[tokio::test]
async fn bounds_success_bodies_and_rejects_invalid_json_shapes() {
    let (provider, _, _server) = fixture(
        json!({"large":"x".repeat(MAX_RESPONSE_BYTES)}),
        StatusCode::OK,
        Duration::ZERO,
    )
    .await;
    assert_eq!(
        provider.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::InvalidResponse
    );
    assert!(provider
        .decode(json!({"status":"completed"}), &request())
        .is_err());
}

#[test]
fn truncated_tool_arguments_preserve_finish_reason_and_usage_without_executable_calls() {
    let provider = OpenAiProvider::new("test", "UNUSED").unwrap();
    let response = provider.decode(json!({
        "status":"incomplete", "incomplete_details":{"reason":"max_output_tokens"},
        "output":[{"type":"function_call", "name":wire_name("search"), "call_id":"partial", "arguments":"{\"path\":"}],
        "usage":{"input_tokens":10, "output_tokens":20, "total_tokens":30}
    }), &request()).unwrap();
    assert_eq!(response.finish_reason, FinishReason::Length);
    assert!(response.tool_calls.is_empty());
    assert!(response.continuation.is_none());
    assert_eq!(response.usage.unwrap().total_tokens, 30);
}
