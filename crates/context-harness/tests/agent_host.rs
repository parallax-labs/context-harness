use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use context_harness::{
    agent_host::{AgentHostBuilder, AgentWorkerOptions},
    agent_model::{
        FinishReason, ModelProvider, ModelProviderCatalog, ModelProviderFactory,
        ModelProviderImplementation, ModelRequest, ModelResponse, ModelResult, ToolCall,
    },
    agent_resource::{Capability, ModelDefinition, ResourceDirectory, ResourceScope},
    agent_runtime::policy::{DenyApprovals, RuntimePolicy},
    agent_store::{BlockedReason, RunOutcome, RunOutcomeKind},
    agent_task_store::{AgentTaskStatus, TaskSchedulingReason},
    config::Config,
    tool_binding::{
        HostToolAuthority, ResolvedToolBinding, RestrictionKind, ToolBindingRequest,
        ToolImplementationCatalog, ToolImplementationDescriptor, ToolImplementationFactory,
        ToolTrustClass,
    },
    traits::{Tool, ToolContext},
};
use serde_json::{json, Value};
use std::time::Duration;
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

struct FailingProviderFactory {
    builds: Arc<AtomicUsize>,
}

struct SlowProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for SlowProvider {
    async fn generate(&self, _request: &ModelRequest) -> ModelResult<ModelResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::future::pending().await
    }
}

struct SlowProviderFactory {
    builds: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl ModelProviderFactory for SlowProviderFactory {
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
        Ok(Arc::new(SlowProvider {
            calls: self.calls.clone(),
        }))
    }
}

impl ModelProviderFactory for FailingProviderFactory {
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
        anyhow::bail!("raw provider detail must not persist")
    }
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
    binds: Arc<AtomicUsize>,
}

#[async_trait]
impl ToolImplementationFactory for EchoFactory {
    fn descriptor(&self) -> &ToolImplementationDescriptor {
        &self.descriptor
    }

    async fn bind(&self, request: ToolBindingRequest) -> Result<Box<dyn Tool>> {
        self.binds.fetch_add(1, Ordering::SeqCst);
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
            ..Default::default()
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

fn failing_model_catalog(builds: Arc<AtomicUsize>) -> ModelProviderCatalog {
    let mut catalog = ModelProviderCatalog::new();
    catalog
        .register(Arc::new(FailingProviderFactory { builds }))
        .unwrap();
    catalog
}

fn slow_model_catalog(builds: Arc<AtomicUsize>, calls: Arc<AtomicUsize>) -> ModelProviderCatalog {
    let mut catalog = ModelProviderCatalog::new();
    catalog
        .register(Arc::new(SlowProviderFactory { builds, calls }))
        .unwrap();
    catalog
}

fn tool_catalog(binds: Arc<AtomicUsize>) -> ToolImplementationCatalog {
    let mut catalog = ToolImplementationCatalog::new();
    catalog
        .register(Arc::new(EchoFactory {
            binds,
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
    let binds = Arc::new(AtomicUsize::new(0));
    let mut authority = HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap();
    authority.enroll_path(temp.path()).unwrap();
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(tools, tool_catalog(binds.clone()), Arc::new(authority))
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );

    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(binds.load(Ordering::SeqCst), 0);
    assert!(!database.exists());
    let host = builder.build("fixture-host").await.unwrap();
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(binds.load(Ordering::SeqCst), 1);
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
async fn task_submission_uses_static_identity_without_building_or_binding() {
    let (temp, config, agents, tools) = fixture();
    let database = config.db.path.clone();
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let binds = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(tools, tool_catalog(binds.clone()), authority)
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );

    let resolved = builder.resolve_task_identity("fixture-host").unwrap();
    assert!(!database.exists());
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert_eq!(binds.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(resolved.agent_name(), "fixture-host");
    assert_eq!(
        resolved.accepted_identity().value()["models"]["fixture"]["implementation_id"],
        "fixture.host-model"
    );
    assert_eq!(
        resolved.accepted_identity().value()["tools"]["fixture-host"][0]["name"],
        "fixture.echo"
    );

    let submitter = builder.build_task_submitter("fixture-host").await.unwrap();
    let result = submitter
        .submit("fixture-request", "Echo hello", Vec::new())
        .await
        .unwrap();
    assert!(matches!(
        result,
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(_)
    ));
    assert!(database.exists());
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert_eq!(binds.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn foreground_worker_links_before_provider_build_and_executes_existing_runtime_path() {
    let (temp, config, agents, tools) = fixture();
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let binds = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(tools, tool_catalog(binds.clone()), authority)
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let submitter = builder
        .clone()
        .build_task_submitter("fixture-host")
        .await
        .unwrap();
    let task = match submitter
        .submit("worker-request", "Echo hello", Vec::new())
        .await
        .unwrap()
    {
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    };
    let store = submitter.store().clone();
    let worker = builder
        .build_worker(AgentWorkerOptions {
            worker_id: Some("fixture-worker".into()),
            lease_duration: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            polling_interval: Duration::from_millis(50),
            graceful_shutdown_timeout: Duration::from_secs(2),
            ..Default::default()
        })
        .await
        .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let running = tokio::spawn(async move { worker.run(shutdown_rx).await });
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = store.get(&task.id).await.unwrap().unwrap();
            if current.status == context_harness::agent_task_store::AgentTaskStatus::Terminal {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown_tx.send(true).unwrap();
    running.await.unwrap().unwrap();

    assert!(terminal.run_id.is_some());
    assert_eq!(terminal.scheduling_reason.as_deref(), Some("run_stopped"));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(binds.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn worker_rejects_identity_drift_before_provider_or_tool_construction() {
    let (temp, config, agents, tools) = fixture();
    let agent_path = agents[0].path.join("fixture.toml");
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let binds = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(tools, tool_catalog(binds.clone()), authority)
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let submitter = builder
        .clone()
        .build_task_submitter("fixture-host")
        .await
        .unwrap();
    let task = match submitter
        .submit("drift-request", "Echo hello", Vec::new())
        .await
        .unwrap()
    {
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    };
    let store = submitter.store().clone();
    let content = fs::read_to_string(&agent_path).unwrap();
    fs::write(
        &agent_path,
        content.replace("Use the fixture tool.", "Changed prompt."),
    )
    .unwrap();
    let worker = builder
        .build_worker(AgentWorkerOptions {
            worker_id: Some("drift-worker".into()),
            lease_duration: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            polling_interval: Duration::from_millis(50),
            graceful_shutdown_timeout: Duration::from_secs(2),
            ..Default::default()
        })
        .await
        .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let running = tokio::spawn(async move { worker.run(shutdown_rx).await });
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = store.get(&task.id).await.unwrap().unwrap();
            if current.status == context_harness::agent_task_store::AgentTaskStatus::Terminal {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown_tx.send(true).unwrap();
    running.await.unwrap().unwrap();

    assert!(terminal.run_id.is_none());
    assert_eq!(
        terminal.scheduling_reason.as_deref(),
        Some("accepted_identity_changed")
    );
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert_eq!(binds.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_setup_failure_is_sanitized_after_atomic_run_link() {
    let (temp, config, agents, tools) = fixture();
    let inspection_config = config.clone();
    let builds = Arc::new(AtomicUsize::new(0));
    let binds = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(config, temp.path(), failing_model_catalog(builds.clone()))
        .unwrap()
        .with_agent_resources(agents)
        .with_tool_bindings(tools, tool_catalog(binds.clone()), authority)
        .with_policy(
            RuntimePolicy {
                allow: vec![Capability::ReadOnly],
                require_approval: vec![],
            },
            Arc::new(DenyApprovals),
        );
    let submitter = builder
        .clone()
        .build_task_submitter("fixture-host")
        .await
        .unwrap();
    let task = match submitter
        .submit("failed-build", "Echo hello", Vec::new())
        .await
        .unwrap()
    {
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    };
    let store = submitter.store().clone();
    let worker = builder
        .build_worker(AgentWorkerOptions {
            worker_id: Some("failure-worker".into()),
            lease_duration: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            polling_interval: Duration::from_millis(50),
            graceful_shutdown_timeout: Duration::from_secs(2),
            ..Default::default()
        })
        .await
        .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let running = tokio::spawn(async move { worker.run(shutdown_rx).await });
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = store.get(&task.id).await.unwrap().unwrap();
            if current.status == context_harness::agent_task_store::AgentTaskStatus::Terminal {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown_tx.send(true).unwrap();
    running.await.unwrap().unwrap();

    let app = context_harness::app_store::SqliteAppStore::connect(&inspection_config)
        .await
        .unwrap();
    let run = app
        .agent_runs(&terminal.workspace_id)
        .unwrap()
        .get_run(terminal.run_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.error.as_deref(), Some("runtime assembly failed"));
    assert!(!run.error.unwrap().contains("raw provider"));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(binds.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn worker_heartbeats_and_gracefully_cancels_owned_execution() {
    let (temp, config, agents, tools) = fixture();
    let inspection_config = config.clone();
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let binds = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        slow_model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(tools, tool_catalog(binds), authority)
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let submitter = builder
        .clone()
        .build_task_submitter("fixture-host")
        .await
        .unwrap();
    let first = match submitter
        .submit("slow-run", "Wait", Vec::new())
        .await
        .unwrap()
    {
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    };
    let second = match submitter
        .submit("slow-run-2", "Wait again", Vec::new())
        .await
        .unwrap()
    {
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    };
    let store = submitter.store().clone();
    let worker = builder
        .build_worker(AgentWorkerOptions {
            worker_id: Some("slow-worker".into()),
            concurrency: 2,
            lease_duration: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            polling_interval: Duration::from_millis(50),
            graceful_shutdown_timeout: Duration::from_secs(3),
            ..Default::default()
        })
        .await
        .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let running = tokio::spawn(async move { worker.run(shutdown_rx).await });
    let started = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let left = store.get(&first.id).await.unwrap().unwrap();
            let right = store.get(&second.id).await.unwrap().unwrap();
            if left.run_id.is_some() && right.run_id.is_some() && calls.load(Ordering::SeqCst) == 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    if started.is_err() {
        let app = context_harness::app_store::SqliteAppStore::connect(&inspection_config)
            .await
            .unwrap();
        let left = store.get(&first.id).await.unwrap().unwrap();
        let right = store.get(&second.id).await.unwrap().unwrap();
        let runs = app.agent_runs(&left.workspace_id).unwrap();
        let left_run = match &left.run_id {
            Some(id) => runs.get_run(id).await.unwrap(),
            None => None,
        };
        let right_run = match &right.run_id {
            Some(id) => runs.get_run(id).await.unwrap(),
            None => None,
        };
        panic!(
            "workers did not both start: left={left:?} right={right:?} left_run={left_run:?} right_run={right_run:?} builds={} calls={}",
            builds.load(Ordering::SeqCst),
            calls.load(Ordering::SeqCst)
        );
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let first_heartbeat = store
                .events(&first.id, 0, 10)
                .await
                .unwrap()
                .iter()
                .any(|event| event.event_type == "task.heartbeat");
            let second_heartbeat = store
                .events(&second.id, 0, 10)
                .await
                .unwrap()
                .iter()
                .any(|event| event.event_type == "task.heartbeat");
            if first_heartbeat && second_heartbeat {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both workers should persist a heartbeat");
    shutdown_tx.send(true).unwrap();
    running.await.unwrap().unwrap();
    for task in [&first, &second] {
        let terminal = store.get(&task.id).await.unwrap().unwrap();
        assert_eq!(
            terminal.status,
            context_harness::agent_task_store::AgentTaskStatus::Terminal
        );
        assert_eq!(terminal.scheduling_reason.as_deref(), Some("run_stopped"));
    }
    assert_eq!(builds.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn durable_cancellation_stops_the_owned_run_before_task_terminalization() {
    let (temp, config, agents, tools) = fixture();
    let inspection_config = config.clone();
    let builds = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        slow_model_catalog(builds.clone(), calls.clone()),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(
        tools,
        tool_catalog(Arc::new(AtomicUsize::new(0))),
        authority,
    )
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let submitter = builder
        .clone()
        .build_task_submitter("fixture-host")
        .await
        .unwrap();
    let task = match submitter
        .submit("durable-cancel", "Wait", Vec::new())
        .await
        .unwrap()
    {
        context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
        _ => panic!("expected created task"),
    };
    let store = submitter.store().clone();
    let worker = builder
        .build_worker(AgentWorkerOptions {
            worker_id: Some("cancel-worker".into()),
            lease_duration: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            polling_interval: Duration::from_millis(50),
            graceful_shutdown_timeout: Duration::from_secs(3),
            ..Default::default()
        })
        .await
        .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let running = tokio::spawn(async move { worker.run(shutdown_rx).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.cancel(&task.id).await.unwrap().status,
        AgentTaskStatus::CancelRequested
    );
    let terminal = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = store.get(&task.id).await.unwrap().unwrap();
            if current.status == AgentTaskStatus::Terminal {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown_tx.send(true).unwrap();
    running.await.unwrap().unwrap();
    let app = context_harness::app_store::SqliteAppStore::connect(&inspection_config)
        .await
        .unwrap();
    let run = app
        .agent_runs(&terminal.workspace_id)
        .unwrap()
        .get_run(terminal.run_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.outcome, Some(RunOutcomeKind::Cancelled));
    assert_eq!(
        terminal.scheduling_reason_kind(),
        Some(TaskSchedulingReason::RunStopped)
    );
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn expired_linked_claims_follow_the_recovery_matrix_without_new_runs() {
    let (temp, config, agents, tools) = fixture();
    let inspection_config = config.clone();
    let builds = Arc::new(AtomicUsize::new(0));
    let authority =
        Arc::new(HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap());
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(builds.clone(), Arc::new(AtomicUsize::new(0))),
    )
    .unwrap()
    .with_agent_resources(agents)
    .with_tool_bindings(
        tools,
        tool_catalog(Arc::new(AtomicUsize::new(0))),
        authority,
    )
    .with_policy(
        RuntimePolicy {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let submitter = builder
        .clone()
        .build_task_submitter("fixture-host")
        .await
        .unwrap();
    let store = submitter.store().clone();
    let app = context_harness::app_store::SqliteAppStore::connect(&inspection_config)
        .await
        .unwrap();
    let mut fixtures = Vec::new();
    for key in [
        "complete",
        "suspended",
        "restart",
        "uncertain",
        "invalid-resume",
    ] {
        let task = match submitter.submit(key, "input", Vec::new()).await.unwrap() {
            context_harness::agent_task_store::AgentTaskSubmissionResult::Created(task) => task,
            _ => panic!("expected created task"),
        };
        let claim = store.claim_next("crashed", 60, 3).await.unwrap().unwrap();
        assert_eq!(claim.task.id, task.id);
        let run = store
            .link_run(
                &claim,
                "agent-v1",
                "fixture",
                context_harness::agent_store::RunBudgets::default(),
            )
            .await
            .unwrap();
        fixtures.push((key, task, run));
    }
    let runs = app.agent_runs(&fixtures[0].1.workspace_id).unwrap();
    runs.finish_run(&fixtures[0].2.id, RunOutcome::Completed("done".into()))
        .await
        .unwrap();
    runs.finish_run(
        &fixtures[1].2.id,
        RunOutcome::Blocked {
            code: BlockedReason::ExternalDependency,
            message: "wait".into(),
        },
    )
    .await
    .unwrap();
    runs.save_checkpoint(&fixtures[3].2.id, 1, 0, &json!({}))
        .await
        .unwrap();
    runs.request_tool(
        &fixtures[3].2.id,
        "uncertain-call",
        "fixture.echo",
        &json!({}),
    )
    .await
    .unwrap();
    runs.save_checkpoint(&fixtures[4].2.id, 1, 0, &json!({}))
        .await
        .unwrap();
    sqlx::query("UPDATE agent_tasks SET lease_expires_at = 0 WHERE workspace_id = ?")
        .bind(&fixtures[0].1.workspace_id)
        .execute(app.pool())
        .await
        .unwrap();

    let worker = builder
        .build_worker(AgentWorkerOptions {
            worker_id: Some("reconciler".into()),
            concurrency: 2,
            lease_duration: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(1),
            polling_interval: Duration::from_millis(50),
            graceful_shutdown_timeout: Duration::from_secs(3),
            ..Default::default()
        })
        .await
        .unwrap();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let running = tokio::spawn(async move { worker.run(shutdown_rx).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut all_terminal = true;
            for (_, task, _) in &fixtures {
                all_terminal &=
                    store.get(&task.id).await.unwrap().unwrap().status == AgentTaskStatus::Terminal;
            }
            if all_terminal {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown_tx.send(true).unwrap();
    running.await.unwrap().unwrap();

    let expected = [
        TaskSchedulingReason::RunStopped,
        TaskSchedulingReason::RunStopped,
        TaskSchedulingReason::RestartRequired,
        TaskSchedulingReason::ReconciliationRequired,
        TaskSchedulingReason::RestartRequired,
    ];
    for ((_, task, original_run), reason) in fixtures.iter().zip(expected) {
        let current = store.get(&task.id).await.unwrap().unwrap();
        assert_eq!(current.scheduling_reason_kind(), Some(reason));
        assert_eq!(current.run_id.as_deref(), Some(original_run.id.as_str()));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_runs")
            .fetch_one(app.pool())
            .await
            .unwrap(),
        5
    );
    assert!(builds.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn worker_options_reject_unbounded_or_incoherent_values_before_initialization() {
    let (temp, config, agents, _tools) = fixture();
    let database = config.db.path.clone();
    let builder = AgentHostBuilder::new(
        config,
        temp.path(),
        model_catalog(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))),
    )
    .unwrap()
    .with_agent_resources(agents);
    let invalid = [
        AgentWorkerOptions {
            concurrency: 0,
            ..Default::default()
        },
        AgentWorkerOptions {
            lease_duration: Duration::from_secs(4),
            ..Default::default()
        },
        AgentWorkerOptions {
            heartbeat_interval: Duration::from_secs(30),
            ..Default::default()
        },
        AgentWorkerOptions {
            polling_interval: Duration::from_millis(49),
            ..Default::default()
        },
        AgentWorkerOptions {
            max_claim_attempts: 101,
            ..Default::default()
        },
        AgentWorkerOptions {
            graceful_shutdown_timeout: Duration::ZERO,
            ..Default::default()
        },
    ];
    for options in invalid {
        assert!(builder.clone().build_worker(options).await.is_err());
    }
    assert!(!database.exists());
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
