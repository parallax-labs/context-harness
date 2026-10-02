use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, FinishReason, ModelRegistry, ModelResponse, ToolCall},
    agent_resource::{
        AgentResource, Capability, LoadedAgentResource, ResourceDirectory, ResourceScope,
    },
    agent_runtime::AgentRuntime,
    config::Config,
    tool_binding::{
        HostToolAuthority, RestrictionKind, ToolBindingRequest, ToolImplementationCatalog,
        ToolImplementationDescriptor, ToolImplementationFactory, ToolTrustClass,
    },
    traits::{Tool, ToolContext},
};
use serde_json::{json, Value};
use std::{fs, sync::Arc};
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

    async fn execute(&self, arguments: Value, _context: &ToolContext) -> Result<Value> {
        self.validate_arguments(&arguments)?;
        let text = arguments["text"].as_str().unwrap();
        if text == "fail" {
            bail!("fixture failure");
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
    let definition = AgentResource::parse(
        r#"
[agent]
name = "compiled-fixture"
model = "test"
tools = ["fixture.echo"]
[agent.execution]
max_turns = 3
timeout_seconds = 10
[prompt]
system = "Use the fixture."
"#,
    )
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
    let mut call = ModelResponse::text("");
    call.finish_reason = FinishReason::ToolCalls;
    call.tool_calls = vec![ToolCall {
        name: "fixture.echo".into(),
        id: "echo-1".into(),
        arguments,
    }];
    call
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
