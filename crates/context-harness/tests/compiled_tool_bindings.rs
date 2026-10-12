use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use context_harness::{
    agent_model::{
        fake::FakeModel, FinishReason, ModelMessage, ModelProvider, ModelRegistry, ModelRequest,
        ModelResponse, ModelResult, ToolCall,
    },
    agent_resource::{
        AgentResource, Capability, LoadedAgentResource, ResourceDirectory, ResourceScope,
    },
    agent_runtime::AgentRuntime,
    config::Config,
    tool_binding::{
        HostToolAuthority, RestrictionKind, ToolBindingRequest, ToolImplementationCatalog,
        ToolImplementationDescriptor, ToolImplementationFactory, ToolTrustClass,
    },
    traits::{Tool, ToolContext, ToolExecutionError},
};
use serde_json::{json, Value};
use std::{
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tempfile::TempDir;
use tokio::sync::watch;

struct EchoFactory {
    descriptor: ToolImplementationDescriptor,
}

impl EchoFactory {
    fn new(capability: Capability) -> Self {
        Self {
            descriptor: ToolImplementationDescriptor {
                id: "rust.fixture.echo".into(),
                version: "fixture-v1".into(),
                implementation_description: "Compiled echo fixture".into(),
                default_public_description: "Echo bounded text".into(),
                config_schema: json!({"type":"object", "properties":{}, "additionalProperties":false}),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "text":{"type":"string", "minLength":1, "maxLength":256},
                        "prefix":{"type":"string", "maxLength":64}
                    },
                    "required":["text", "prefix"],
                    "additionalProperties":false
                }),
                capabilities: vec![capability],
                supported_restrictions: vec![RestrictionKind::MaxOutputBytes],
                trust_class: ToolTrustClass::Compiled,
            },
        }
    }
}

#[async_trait]
impl ToolImplementationFactory for EchoFactory {
    fn descriptor(&self) -> &ToolImplementationDescriptor {
        &self.descriptor
    }

    async fn bind(&self, request: ToolBindingRequest) -> Result<Box<dyn Tool>> {
        Ok(Box::new(EchoTool {
            binding: request.resolved,
        }))
    }
}

struct EchoTool {
    binding: context_harness::tool_binding::ResolvedToolBinding,
}

#[async_trait]
impl Tool for EchoTool {
    fn capabilities(&self) -> Option<Vec<Capability>> {
        Some(self.binding.capabilities.clone())
    }

    fn binding_metadata(&self) -> Option<Value> {
        Some(json!({
            "binding_version": self.binding.binding_version,
            "implementation_id": self.binding.implementation_id,
            "implementation_version": self.binding.implementation_version,
            "trust_class": self.binding.trust_class,
        }))
    }

    fn name(&self) -> &str {
        &self.binding.name
    }

    fn description(&self) -> &str {
        &self.binding.description
    }

    fn parameters_schema(&self) -> Value {
        self.binding.public_schema.clone()
    }

    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        let object = arguments
            .as_object()
            .context("arguments must be an object")?;
        ensure!(object.len() == 1, "unexpected argument");
        let text = object
            .get("text")
            .and_then(Value::as_str)
            .context("text is required")?;
        ensure!(!text.is_empty() && text.len() <= 256, "invalid text");
        Ok(())
    }

    fn approval_arguments(&self, arguments: &Value) -> Result<Value> {
        self.validate_arguments(arguments)?;
        Ok(json!({
            "text": arguments["text"],
            "prefix": self.binding.fixed["prefix"],
        }))
    }

    async fn execute(&self, arguments: Value, _context: &ToolContext) -> Result<Value> {
        self.validate_arguments(&arguments)?;
        let text = arguments["text"].as_str().unwrap();
        if text == "fail" {
            bail!("fixture failure");
        }
        if text.starts_with("recover") {
            let code = if text == "recover_alt_code" {
                "unavailable"
            } else {
                "not_found"
            };
            return Err(ToolExecutionError::recoverable(
                code,
                "The requested fixture value does not exist.",
            )?
            .into());
        }
        if text == "terminal" {
            return Err(ToolExecutionError::terminal(
                "fixture_failed",
                "The fixture could not complete the request.",
            )?
            .into());
        }
        if text == "uncertain" {
            return Err(ToolExecutionError::uncertain_side_effect(
                "commit_unknown",
                "The fixture cannot confirm whether the operation completed.",
            )?
            .into());
        }
        if text == "block" {
            std::future::pending::<()>().await;
        }
        let prefix = self.binding.fixed["prefix"]
            .as_str()
            .context("fixed prefix is missing")?;
        let output = json!({"echo": format!("{prefix}{text}")});
        let limit = self
            .binding
            .restrictions
            .max_output_bytes
            .unwrap_or(1024 * 1024);
        ensure!(
            serde_json::to_vec(&output)?.len() as u64 <= limit,
            "output limit exceeded"
        );
        Ok(output)
    }
}

const RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "fixture.echo"
implementation = "rust.fixture.echo"
[fixed]
prefix = "fixture: "
[restrictions]
max_output_bytes = 128
"#;

fn agent() -> LoadedAgentResource {
    agent_with_turns(3)
}

fn agent_with_turns(max_turns: u32) -> LoadedAgentResource {
    let definition = AgentResource::parse(&format!(
        r#"
[agent]
name = "compiled-fixture"
model = "test"
tools = ["fixture.echo"]
[agent.execution]
max_turns = {max_turns}
timeout_seconds = 10
[prompt]
system = "Use the fixture."
"#
    ))
    .unwrap();
    LoadedAgentResource {
        path: "agent.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    }
}

fn catalog(capability: Capability) -> ToolImplementationCatalog {
    let mut catalog = ToolImplementationCatalog::new();
    catalog
        .register(Arc::new(EchoFactory::new(capability)))
        .unwrap();
    catalog
}

fn tool_call(arguments: Value) -> ModelResponse {
    tool_call_id("echo-1", arguments)
}

fn tool_call_id(id: &str, arguments: Value) -> ModelResponse {
    let mut call = ModelResponse::text("");
    call.finish_reason = FinishReason::ToolCalls;
    call.tool_calls = vec![ToolCall {
        name: "fixture.echo".into(),
        id: id.into(),
        arguments,
    }];
    call
}

async fn runtime_with_responses(
    temp: &TempDir,
    responses: impl IntoIterator<Item = context_harness::agent_model::ModelResult<ModelResponse>>,
) -> AgentRuntime {
    runtime_with_provider(temp, Arc::new(FakeModel::new(responses))).await
}

async fn runtime_with_provider(temp: &TempDir, provider: Arc<dyn ModelProvider>) -> AgentRuntime {
    let tools = temp.path().join("tools");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    let mut models = ModelRegistry::default();
    models
        .register("test", "fake", "fixture", provider)
        .unwrap();
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    AgentRuntime::new(config, temp.path(), models)
        .await
        .unwrap()
        .with_tool_binding_catalog(&directories, &catalog(Capability::ReadOnly), authority)
        .await
        .unwrap()
}

struct RecoverThenBlock(AtomicUsize);

#[async_trait]
impl ModelProvider for RecoverThenBlock {
    async fn generate(&self, _request: &ModelRequest) -> ModelResult<ModelResponse> {
        if self.0.fetch_add(1, Ordering::SeqCst) > 0 {
            std::future::pending().await
        }
        Ok(tool_call_id("recover-1", json!({"text":"recover"})))
    }
}

struct VerifyRecovered;

#[async_trait]
impl ModelProvider for VerifyRecovered {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse> {
        let ModelMessage::Tool { call_id, content } = request.messages.last().unwrap() else {
            panic!("recoverable observation missing")
        };
        assert_eq!(call_id, "recover-1");
        assert_eq!(
            content,
            r#"{"ok":false,"error":{"type":"recoverable_tool_error","code":"not_found","message":"The requested fixture value does not exist."}}"#
        );
        Ok(ModelResponse::text("resumed"))
    }
}

async fn wait_for_event(runtime: &AgentRuntime, event_type: &str, count: usize) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(run) = runtime.store().history(1).await.unwrap().first() {
                let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
                if events
                    .iter()
                    .filter(|event| event.event_type == event_type)
                    .count()
                    >= count
                {
                    return run.id.clone();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn compiled_tool_recoverable_error_is_exact_and_model_visible() {
    let temp = TempDir::new().unwrap();
    let runtime = runtime_with_responses(
        &temp,
        [
            Ok(tool_call_id("recover-1", json!({"text":"recover"}))),
            Ok(ModelResponse::text("recovered")),
        ],
    )
    .await;
    let (_sender, cancel) = watch::channel(false);
    let run = runtime.run(&agent(), "Recover", cancel).await.unwrap();
    assert_eq!(run.status, "completed");
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls[0].outcome_class.as_deref(), Some("recoverable_error"));
    assert_eq!(calls[0].error_code.as_deref(), Some("not_found"));
    assert_eq!(calls[0].consecutive_count, Some(1));
    assert_eq!(
        calls[0].result_content.as_deref(),
        Some(
            r#"{"ok":false,"error":{"type":"recoverable_tool_error","code":"not_found","message":"The requested fixture value does not exist."}}"#
        )
    );
    let checkpoint = runtime
        .store()
        .latest_checkpoint(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(checkpoint.state.to_string().contains(r#"{\"ok\":false,\"error\":{\"type\":\"recoverable_tool_error\",\"code\":\"not_found\",\"message\":\"The requested fixture value does not exist.\"}}"#));
    assert_eq!(run.usage.model_turns, 2);
}

#[tokio::test]
async fn recoverable_observation_resumes_without_replaying_the_tool() {
    let temp = TempDir::new().unwrap();
    let runtime = Arc::new(
        runtime_with_provider(&temp, Arc::new(RecoverThenBlock(AtomicUsize::new(0)))).await,
    );
    let resource = agent_with_turns(5);
    let run_resource = resource.clone();
    let running = runtime.clone();
    let (_sender, cancel) = watch::channel(false);
    let task = tokio::spawn(async move { running.run(&run_resource, "Recover", cancel).await });
    let run_id = wait_for_event(&runtime, "model.requested", 2).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    let resumed_runtime = runtime_with_provider(&temp, Arc::new(VerifyRecovered)).await;
    let (_sender, cancel) = watch::channel(false);
    let resumed = resumed_runtime
        .resume(&run_id, &resource, cancel)
        .await
        .unwrap();
    assert_eq!(resumed.output.as_deref(), Some("resumed"));
    let calls = resumed_runtime
        .store()
        .tool_invocations(&run_id)
        .await
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].consecutive_count, Some(1));
}

#[tokio::test]
async fn third_identical_recoverable_error_stops_before_another_model_call() {
    let temp = TempDir::new().unwrap();
    let runtime = runtime_with_responses(
        &temp,
        [
            Ok(tool_call_id("recover-1", json!({"text":"recover"}))),
            Ok(tool_call_id("recover-2", json!({"text":"recover"}))),
            Ok(tool_call_id("recover-3", json!({"text":"recover"}))),
        ],
    )
    .await;
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&agent_with_turns(5), "Recover", cancel)
        .await
        .unwrap();
    assert_eq!(run.reason_code.as_deref(), Some("tool_loop_detected"));
    assert_eq!(run.usage.model_turns, 3);
    let detail = &run.reason_detail.as_ref().unwrap().0;
    assert_eq!(detail["consecutive_count"], 3);
    assert_eq!(detail["tool"], "fixture.echo");
    assert!(detail.get("message").is_none());
    assert!(detail.get("arguments").is_none());
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.consecutive_count)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(2), Some(3)]
    );
}

#[tokio::test]
async fn changed_arguments_and_success_reset_recoverable_repetition() {
    let temp = TempDir::new().unwrap();
    let runtime = runtime_with_responses(
        &temp,
        [
            Ok(tool_call_id("recover-1", json!({"text":"recover"}))),
            Ok(tool_call_id("recover-2", json!({"text":"recover-other"}))),
            Ok(tool_call_id("success", json!({"text":"hello"}))),
            Ok(tool_call_id("recover-3", json!({"text":"recover"}))),
            Ok(ModelResponse::text("done")),
        ],
    )
    .await;
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&agent_with_turns(6), "Recover", cancel)
        .await
        .unwrap();
    assert_eq!(run.status, "completed");
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.consecutive_count)
            .collect::<Vec<_>>(),
        vec![Some(1), Some(1), None, Some(1)]
    );
    assert_ne!(calls[0].repetition_key, calls[1].repetition_key);
    assert_eq!(calls[0].repetition_key, calls[3].repetition_key);
}

#[tokio::test]
async fn changed_error_code_starts_a_new_recoverable_sequence() {
    let temp = TempDir::new().unwrap();
    let runtime = runtime_with_responses(
        &temp,
        [
            Ok(tool_call_id("recover-1", json!({"text":"recover"}))),
            Ok(tool_call_id(
                "recover-2",
                json!({"text":"recover_alt_code"}),
            )),
            Ok(ModelResponse::text("done")),
        ],
    )
    .await;
    let (_sender, cancel) = watch::channel(false);
    let run = runtime
        .run(&agent_with_turns(4), "Recover", cancel)
        .await
        .unwrap();
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls[0].consecutive_count, Some(1));
    assert_eq!(calls[1].consecutive_count, Some(1));
    assert_ne!(calls[0].repetition_key, calls[1].repetition_key);
}

#[tokio::test]
async fn typed_terminal_and_uncertain_errors_remain_non_model_visible() {
    for (text, class, code) in [
        ("terminal", "terminal_error", "fixture_failed"),
        ("uncertain", "uncertain_side_effect", "commit_unknown"),
    ] {
        let temp = TempDir::new().unwrap();
        let runtime =
            runtime_with_responses(&temp, [Ok(tool_call_id("failed-1", json!({"text":text})))])
                .await;
        let (_sender, cancel) = watch::channel(false);
        let run = runtime.run(&agent(), "Fail", cancel).await.unwrap();
        assert_eq!(run.reason_code.as_deref(), Some("tool_error"));
        assert_eq!(run.usage.model_turns, 1);
        let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
        assert_eq!(calls[0].outcome_class.as_deref(), Some(class));
        assert_eq!(calls[0].error_code.as_deref(), Some(code));
        assert!(calls[0].result.is_none());
    }
}

#[tokio::test]
async fn registered_compiled_tool_uses_common_runtime_without_name_dispatch() {
    let temp = TempDir::new().unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    let provider = Arc::new(FakeModel::new([
        Ok(tool_call(json!({"text":"hello"}))),
        Ok(ModelResponse::text("done")),
    ]));
    let mut models = ModelRegistry::default();
    models
        .register("test", "fake", "fixture", provider)
        .unwrap();
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let runtime = AgentRuntime::new(config, temp.path(), models)
        .await
        .unwrap()
        .with_tool_binding_catalog(&directories, &catalog(Capability::ReadOnly), authority)
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime.run(&agent(), "Echo", cancel).await.unwrap();
    assert_eq!(run.status, "completed");
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0]
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("fixture: hello"));
}

#[tokio::test]
async fn compiled_tool_validation_failure_and_output_bounds_are_durable() {
    for (arguments, expected_status) in [
        (json!({"unknown":"value"}), "denied"),
        (json!({"text":"fail"}), "failed"),
        (json!({"text":"x".repeat(256)}), "failed"),
    ] {
        let temp = TempDir::new().unwrap();
        let tools = temp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
        let directories = [ResourceDirectory {
            path: tools,
            scope: ResourceScope::Workspace,
        }];
        let mut config = Config::minimal();
        config.db.path = temp.path().join("ctx.sqlite");
        let provider = Arc::new(FakeModel::new([Ok(tool_call(arguments))]));
        let mut models = ModelRegistry::default();
        models
            .register("test", "fake", "fixture", provider)
            .unwrap();
        let authority =
            Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
        let runtime = AgentRuntime::new(config, temp.path(), models)
            .await
            .unwrap()
            .with_tool_binding_catalog(&directories, &catalog(Capability::ReadOnly), authority)
            .await
            .unwrap();
        let (_sender, cancel) = watch::channel(false);
        let run = runtime.run(&agent(), "Echo", cancel).await.unwrap();
        assert_eq!(run.status, "failed");
        let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
        assert_eq!(calls[0].status, expected_status);
    }
}

#[tokio::test]
async fn cancelling_compiled_tool_cleans_up_the_started_invocation() {
    let temp = TempDir::new().unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    let provider = Arc::new(FakeModel::new([Ok(tool_call(json!({"text":"block"})))]));
    let mut models = ModelRegistry::default();
    models
        .register("test", "fake", "fixture", provider)
        .unwrap();
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let runtime = Arc::new(
        AgentRuntime::new(config, temp.path(), models)
            .await
            .unwrap()
            .with_tool_binding_catalog(&directories, &catalog(Capability::ReadOnly), authority)
            .await
            .unwrap(),
    );
    let (sender, cancel) = watch::channel(false);
    let running = runtime.clone();
    let task = tokio::spawn(async move { running.run(&agent(), "Echo", cancel).await });
    let run_id = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(run) = runtime.store().history(1).await.unwrap().first() {
                let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
                if calls.first().is_some_and(|call| call.status == "started") {
                    break run.id.clone();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    sender.send(true).unwrap();
    let run = task.await.unwrap().unwrap();
    assert_eq!(run.status, "cancelled");
    let calls = runtime.store().tool_invocations(&run_id).await.unwrap();
    assert_eq!(calls[0].status, "failed");
    assert_eq!(calls[0].error.as_deref(), Some("run interrupted"));
}

#[tokio::test]
async fn host_authority_rejects_ungranted_compiled_capability_before_model_use() {
    let temp = TempDir::new().unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    let models = ModelRegistry::default();
    let runtime = AgentRuntime::new(config, temp.path(), models)
        .await
        .unwrap();
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let error = runtime
        .with_tool_binding_catalog(
            &directories,
            &catalog(Capability::ProcessExecute),
            authority,
        )
        .await
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("requires capability not granted by the host"));
}
