use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use context_harness::{
    agent_host::AgentHostBuilder,
    agent_model::{
        FinishReason, ModelProvider, ModelProviderCatalog, ModelProviderFactory,
        ModelProviderImplementation, ModelRequest, ModelResponse, ModelResult, ToolCall,
    },
    agent_resource::{Capability, ModelDefinition, ResourceDirectory, ResourceScope},
    agent_runtime::policy::{DenyApprovals, RuntimePolicy},
    config::Config,
    tool_binding::{
        HostToolAuthority, ResolvedToolBinding, RestrictionKind, ToolBindingRequest,
        ToolImplementationCatalog, ToolImplementationDescriptor, ToolImplementationFactory,
        ToolTrustClass,
    },
    traits::{Tool, ToolContext},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tempfile::TempDir;
use tokio::sync::watch;

struct FixtureProvider {
    responses: Mutex<VecDeque<ModelResponse>>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for FixtureProvider {
    async fn generate(&self, _request: &ModelRequest) -> ModelResult<ModelResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.responses.lock().unwrap().pop_front().unwrap())
    }
}

struct FixtureProviderFactory {
    builds: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl ModelProviderFactory for FixtureProviderFactory {
    fn provider_name(&self) -> &str {
        "fixture"
    }

    fn implementation(&self) -> ModelProviderImplementation {
        ModelProviderImplementation::new("fixture.host-model", "1")
    }

    fn validate(&self, _definition: &ModelDefinition) -> Result<()> {
        Ok(())
    }

    fn build(&self, _definition: &ModelDefinition) -> Result<Arc<dyn ModelProvider>> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        let mut call = ModelResponse::text("");
        call.finish_reason = FinishReason::ToolCalls;
        call.tool_calls = vec![ToolCall {
            id: "echo-1".into(),
            name: "fixture.echo".into(),
            arguments: json!({"text":"hello"}),
        }];
        Ok(Arc::new(FixtureProvider {
            responses: Mutex::new(VecDeque::from([call, ModelResponse::text("done")])),
            calls: self.calls.clone(),
        }))
    }
}

struct EchoFactory {
    descriptor: ToolImplementationDescriptor,
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
    binding: ResolvedToolBinding,
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
        let text = arguments
            .get("text")
            .and_then(Value::as_str)
            .context("text is required")?;
        ensure!(!text.is_empty(), "text is required");
        Ok(())
    }

    async fn execute(&self, arguments: Value, _context: &ToolContext) -> Result<Value> {
        self.validate_arguments(&arguments)?;
        Ok(json!({"echo": arguments["text"]}))
    }
}

fn fixture() -> (
    TempDir,
    Config,
    Vec<ResourceDirectory>,
    Vec<ResourceDirectory>,
) {
    let temp = TempDir::new().unwrap();
    let agents = temp.path().join("agents");
    let tools = temp.path().join("tools");
    fs::create_dir(&agents).unwrap();
    fs::create_dir(&tools).unwrap();
    fs::write(
        agents.join("fixture.toml"),
        r#"
[agent]
name = "fixture-host"
model = "fixture"
tools = ["fixture.echo"]
[agent.execution]
max_turns = 3
timeout_seconds = 10
[prompt]
system = "Use the fixture tool."
"#,
    )
    .unwrap();
    fs::write(
        tools.join("echo.toml"),
        r#"
schema_version = 1
[tool]
name = "fixture.echo"
implementation = "rust.fixture.echo"
"#,
    )
    .unwrap();
    let mut config = Config::minimal();
    config.db.path = temp.path().join("ctx.sqlite");
    config.models.insert(
        "fixture".into(),
        ModelDefinition {
            provider: "fixture".into(),
            model: "fixture-model".into(),
            api_key_env: None,
        },
    );
    let directory = |path| ResourceDirectory {
        path,
        scope: ResourceScope::Workspace,
    };
    (
        temp,
        config,
        vec![directory(agents)],
        vec![directory(tools)],
    )
}

fn model_catalog(builds: Arc<AtomicUsize>, calls: Arc<AtomicUsize>) -> ModelProviderCatalog {
    let mut catalog = ModelProviderCatalog::new();
    catalog
        .register(Arc::new(FixtureProviderFactory { builds, calls }))
        .unwrap();
    catalog
}

fn tool_catalog() -> ToolImplementationCatalog {
    let mut catalog = ToolImplementationCatalog::new();
    catalog
        .register(Arc::new(EchoFactory {
            descriptor: ToolImplementationDescriptor {
                id: "rust.fixture.echo".into(),
                version: "1".into(),
                implementation_description: "Fixture compiled echo".into(),
                default_public_description: "Echo text".into(),
                config_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
                input_schema: json!({
                    "type":"object",
                    "properties":{"text":{"type":"string","minLength":1}},
                    "required":["text"],
                    "additionalProperties":false
                }),
                capabilities: vec![Capability::ReadOnly],
                supported_restrictions: vec![RestrictionKind::MaxOutputBytes],
                trust_class: ToolTrustClass::Compiled,
            },
        }))
        .unwrap();
    catalog
}

#[tokio::test]
async fn external_host_composes_registered_model_and_tool_factories() {
    let (temp, config, agents, tools) = fixture();
    let database = config.db.path.clone();
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut authority = HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap();
    authority.enroll_path(temp.path()).unwrap();
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(tools, tool_catalog(), Arc::new(authority))
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );

    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!database.exists());
    let host = builder.build("fixture-host").await.unwrap();
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let (_sender, cancel) = watch::channel(false);
    let run = host.run("Echo hello", cancel).await.unwrap();
    assert_eq!(run.status, "completed");
    assert_eq!(run.output.as_deref(), Some("done"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        host.store().tool_invocations(&run.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn host_builder_grants_no_implicit_tool_authority() {
    let (temp, config, agents, _tools) = fixture();
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let host = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .build("fixture-host")
    .await
    .unwrap();

    let (_sender, cancel) = watch::channel(false);
    let run = host.run("Echo hello", cancel).await.unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
