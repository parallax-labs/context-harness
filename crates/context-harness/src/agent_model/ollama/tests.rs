use super::*;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use std::sync::{Arc, Mutex};

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![ModelMessage::User {
            content: "Question".into(),
        }],
        ..Default::default()
    }
}

fn definition(base_url: String) -> ModelDefinition {
    ModelDefinition {
        provider: "ollama".into(),
        model: "qwen3".into(),
        base_url: Some(base_url),
        ..Default::default()
    }
}

#[derive(Clone)]
struct Fixture {
    status: StatusCode,
    response: Vec<u8>,
    delay: Duration,
    captured: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
}

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn fixture(status: StatusCode, response: Value) -> (OllamaProvider, Fixture, Server) {
    fixture_bytes(
        status,
        serde_json::to_vec(&response).unwrap(),
        Duration::ZERO,
    )
    .await
}

async fn fixture_bytes(
    status: StatusCode,
    response: Vec<u8>,
    delay: Duration,
) -> (OllamaProvider, Fixture, Server) {
    let fixture = Fixture {
        status,
        response,
        delay,
        captured: Arc::new(Mutex::new(Vec::new())),
    };
    async fn handle(
        State(state): State<Fixture>,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        let body = serde_json::from_slice(&body).unwrap();
        state.captured.lock().unwrap().push((headers, body));
        tokio::time::sleep(state.delay).await;
        (state.status, state.response)
    }
    let app = Router::new()
        .route("/api/chat", post(handle))
        .with_state(fixture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = Server(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    (
        OllamaProvider::from_definition(&definition(base_url)).unwrap(),
        fixture,
        server,
    )
}

fn answer(text: &str) -> Value {
    json!({
        "model":"qwen3", "done":true, "done_reason":"stop",
        "message":{"role":"assistant", "content":text},
        "prompt_eval_count":11, "eval_count":4
    })
}

#[test]
fn validates_and_canonicalizes_local_only_configuration_offline() {
    let default = ModelDefinition {
        provider: "ollama".into(),
        model: "qwen3".into(),
        ..Default::default()
    };
    assert_eq!(
        effective_config(&default).unwrap(),
        ("http://127.0.0.1:11434".into(), 120)
    );
    for base_url in [
        "https://127.0.0.1:11434",
        "http://localhost:11434",
        "http://192.168.1.2:11434",
        "http://127.0.0.1:11434/api",
        "http://user@127.0.0.1:11434",
        "http://127.0.0.1:11434?query=1",
    ] {
        assert!(validate_definition(&definition(base_url.into())).is_err());
    }
    let mut invalid = default.clone();
    invalid.api_key_env = Some("OLLAMA_API_KEY".into());
    assert!(validate_definition(&invalid).is_err());
    invalid.api_key_env = None;
    invalid.timeout_seconds = Some(0);
    assert!(validate_definition(&invalid).is_err());
}

#[tokio::test]
async fn sends_native_chat_and_maps_text_usage_without_authentication() {
    let (provider, fixture, _server) = fixture(StatusCode::OK, answer("Hello")).await;
    let response = provider.generate(&request()).await.unwrap();
    assert_eq!(response.text, "Hello");
    assert_eq!(response.finish_reason, FinishReason::Completed);
    assert_eq!(response.usage.unwrap().total_tokens, 15);
    assert!(response.continuation.is_none());
    let captured = fixture.captured.lock().unwrap();
    let (headers, body) = &captured[0];
    assert!(!headers.contains_key(reqwest::header::AUTHORIZATION));
    assert_eq!(body["model"], "qwen3");
    assert_eq!(body["stream"], false);
    assert_eq!(body["think"], false);
    assert_eq!(body["messages"][0]["content"], "Question");
}

#[tokio::test]
async fn maps_tools_results_schema_limits_and_deterministic_call_ids() {
    let mut tool_response = answer("");
    tool_response["message"]["tool_calls"] = json!([{"type":"function", "function":{
        "name":wire_name("workspace.read"), "arguments":{"path":"src/main.rs"}
    }}]);
    let (provider, fixture, _server) = fixture(StatusCode::OK, tool_response).await;
    let mut req = request();
    req.tools.push(ModelTool {
        name: "workspace.read".into(),
        description: "Read files".into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
    });
    req.output_schema = Some(OutputSchema {
        name: "result".into(),
        schema: json!({"type":"object"}),
    });
    req.max_output_tokens = Some(128);
    let response = provider.generate(&req).await.unwrap();
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    assert_eq!(response.tool_calls[0].id, "ollama-call-0-0");
    assert_eq!(response.tool_calls[0].name, "workspace.read");
    let first_body = &fixture.captured.lock().unwrap()[0].1;
    assert_eq!(first_body["format"], json!({"type":"object"}));
    assert_eq!(first_body["options"]["num_predict"], 128);
    assert_eq!(
        first_body["tools"][0]["function"]["name"],
        wire_name("workspace.read")
    );

    req.output_schema = None;
    req.messages.push(response.message());
    req.messages.push(ModelMessage::Tool {
        call_id: "ollama-call-0-0".into(),
        content: "file contents".into(),
    });
    let body: Value = serde_json::from_slice(&provider.body(&req).unwrap()).unwrap();
    assert_eq!(body["messages"][1]["role"], "assistant");
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(
        body["messages"][2]["tool_name"],
        wire_name("workspace.read")
    );
}

#[tokio::test]
async fn maps_structured_output_length_and_missing_usage() {
    let mut structured = answer("{\"ok\":true}");
    structured
        .as_object_mut()
        .unwrap()
        .remove("prompt_eval_count");
    structured.as_object_mut().unwrap().remove("eval_count");
    let (provider, _, _server) = fixture(StatusCode::OK, structured).await;
    let mut req = request();
    req.output_schema = Some(OutputSchema {
        name: "result".into(),
        schema: json!({"type":"object"}),
    });
    let response = provider.generate(&req).await.unwrap();
    assert_eq!(response.structured_output, Some(json!({"ok":true})));
    assert!(response.usage.is_none());

    let mut length = answer("partial");
    length["done_reason"] = json!("length");
    let (provider, _, _server) = fixture(StatusCode::OK, length).await;
    let response = provider.generate(&request()).await.unwrap();
    assert_eq!(response.finish_reason, FinishReason::Length);
}

#[tokio::test]
async fn classifies_safe_errors_and_never_exposes_raw_bodies() {
    for (status, message, expected) in [
        (
            404,
            "model secret-name not found",
            ModelErrorKind::ModelUnavailable,
        ),
        (429, "slow down secret", ModelErrorKind::RateLimited),
        (503, "backend secret", ModelErrorKind::Unavailable),
        (
            400,
            "qwen3 does not support tools secret",
            ModelErrorKind::UnsupportedCapability,
        ),
        (422, "bad request secret", ModelErrorKind::InvalidRequest),
    ] {
        let (provider, fixture, _server) = fixture(
            StatusCode::from_u16(status).unwrap(),
            json!({"error":message}),
        )
        .await;
        let error = provider.generate(&request()).await.unwrap_err();
        assert_eq!(error.kind, expected);
        assert_eq!(error.http_status, Some(status));
        assert!(!format!("{error:?} {error}").contains("secret"));
        assert_eq!(fixture.captured.lock().unwrap().len(), 1);
    }
    let (provider, _, _server) =
        fixture_bytes(StatusCode::OK, b"not-json".to_vec(), Duration::ZERO).await;
    assert_eq!(
        provider.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::InvalidResponse
    );
}

#[tokio::test]
async fn refuses_redirects_timeouts_unavailable_endpoints_and_oversized_bodies() {
    async fn redirect() -> impl IntoResponse {
        let mut headers = HeaderMap::new();
        headers.insert(
            "location",
            HeaderValue::from_static("http://127.0.0.1:9/escape"),
        );
        (StatusCode::TEMPORARY_REDIRECT, headers, "")
    }
    async fn escaped() -> StatusCode {
        panic!("redirect must not be followed")
    }
    let app = Router::new()
        .route("/api/chat", post(redirect))
        .route("/escape", get(escaped));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = OllamaProvider::from_definition(&definition(format!(
        "http://{}",
        listener.local_addr().unwrap()
    )))
    .unwrap();
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let redirect_error = provider.generate(&request()).await.unwrap_err();
    assert_eq!(redirect_error.kind, ModelErrorKind::ProviderFailure);
    assert_eq!(redirect_error.http_status, Some(307));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable = definition(format!("http://{}", listener.local_addr().unwrap()));
    drop(listener);
    assert_eq!(
        OllamaProvider::from_definition(&unavailable)
            .unwrap()
            .generate(&request())
            .await
            .unwrap_err()
            .kind,
        ModelErrorKind::Unavailable
    );

    let mut huge = request();
    huge.messages = vec![ModelMessage::User {
        content: "x".repeat(MAX_BODY_BYTES),
    }];
    assert_eq!(
        provider.generate(&huge).await.unwrap_err().kind,
        ModelErrorKind::InvalidRequest
    );

    let (mut slow, _, _server) = fixture_bytes(
        StatusCode::OK,
        serde_json::to_vec(&answer("late")).unwrap(),
        Duration::from_millis(200),
    )
    .await;
    slow.client = Client::builder()
        .timeout(Duration::from_millis(20))
        .redirect(Policy::none())
        .build()
        .unwrap();
    assert_eq!(
        slow.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::Timeout
    );
}

#[tokio::test]
async fn rejects_oversized_responses_and_invalid_finish_states() {
    let (provider, _, _server) = fixture_bytes(
        StatusCode::OK,
        vec![b'x'; MAX_BODY_BYTES + 1],
        Duration::ZERO,
    )
    .await;
    assert_eq!(
        provider.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::InvalidResponse
    );
    let (provider, _, _server) = fixture(
        StatusCode::OK,
        json!({"model":"qwen3","done":false,"message":{"role":"assistant","content":"partial"}}),
    )
    .await;
    assert_eq!(
        provider.generate(&request()).await.unwrap_err().kind,
        ModelErrorKind::InvalidResponse
    );
}

#[tokio::test]
async fn dropping_an_in_flight_call_cancels_without_a_provider_error() {
    let (provider, fixture, _server) = fixture_bytes(
        StatusCode::OK,
        serde_json::to_vec(&answer("late")).unwrap(),
        Duration::from_secs(5),
    )
    .await;
    let call = tokio::spawn(async move { provider.generate(&request()).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if !fixture.captured.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());
}
