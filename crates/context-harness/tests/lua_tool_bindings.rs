use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, FinishReason, ModelRegistry, ModelResponse, ToolCall},
    agent_resource::{
        AgentResource, Capability, LoadedAgentResource, ResourceDirectory, ResourceScope,
    },
    agent_runtime::{
        policy::{ApprovalHandler, ApprovalRequest, RuntimePolicy},
        AgentRuntime,
    },
    config::Config,
    lua_tool_binding::{LuaToolFactory, LuaToolManifest},
    tool_binding::{HostToolAuthority, ToolImplementationCatalog},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tempfile::TempDir;
use tokio::sync::watch;

const RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "fixture.lua_echo"
implementation = "lua.fixture.echo"
[fixed]
prefix = "lua: "
[restrictions]
max_output_bytes = 256
"#;

fn config(root: &Path) -> Config {
    let mut config = Config::minimal();
    config.db.path = root.join("ctx.sqlite");
    config
}

fn manifest(script_path: &Path) -> LuaToolManifest {
    LuaToolManifest {
        implementation_id: "lua.fixture.echo".into(),
        script_path: script_path.into(),
        implementation_description: "Privileged Lua echo fixture".into(),
        default_public_description: "Echo through Lua".into(),
        config_schema: json!({"type":"object", "properties":{}, "additionalProperties":false}),
        input_schema: json!({
            "type":"object",
            "properties":{
                "text":{"type":"string"},
                "prefix":{"type":"string"}
            },
            "required":["text", "prefix"],
            "additionalProperties":false
        }),
        capabilities: vec![
            Capability::ReadOnly,
            Capability::Network,
            Capability::ExternalSideEffect,
        ],
        permitted_host_apis: [
            "base64", "context", "crypto", "env", "fs", "http", "json", "log", "sleep",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        timeout_seconds: 5,
        max_output_bytes: 256,
    }
}

fn agent() -> LoadedAgentResource {
    let definition = AgentResource::parse(
        r#"
[agent]
name = "lua-fixture"
model = "test"
tools = ["fixture.lua_echo"]
[agent.execution]
max_turns = 3
timeout_seconds = 10
[agent.permissions]
allow = ["read_only", "network"]
require_approval = ["external_side_effect"]
[prompt]
system = "Use the Lua fixture."
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

fn policy() -> RuntimePolicy {
    RuntimePolicy {
        allow: vec![Capability::ReadOnly, Capability::Network],
        require_approval: vec![Capability::ExternalSideEffect],
    }
}

fn catalog(script: &Path, config: Arc<Config>) -> ToolImplementationCatalog {
    let mut catalog = ToolImplementationCatalog::new();
    catalog
        .register(Arc::new(
            LuaToolFactory::from_manifest(manifest(script), config).unwrap(),
        ))
        .unwrap();
    catalog
}

fn authority(root: &Path) -> Arc<HostToolAuthority> {
    Arc::new(
        HostToolAuthority::new(
            root,
            vec![
                Capability::ReadOnly,
                Capability::Network,
                Capability::ExternalSideEffect,
            ],
        )
        .unwrap(),
    )
}

fn tool_call(arguments: Value) -> ModelResponse {
    let mut call = ModelResponse::text("");
    call.finish_reason = FinishReason::ToolCalls;
    call.tool_calls = vec![ToolCall {
        name: "fixture.lua_echo".into(),
        id: "lua-1".into(),
        arguments,
    }];
    call
}

struct Grant {
    count: AtomicUsize,
    arguments: Mutex<Vec<Value>>,
}

#[async_trait]
impl ApprovalHandler for Grant {
    async fn approve(&self, request: &ApprovalRequest) -> bool {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.arguments
            .lock()
            .unwrap()
            .push(request.arguments.clone());
        true
    }
}

#[tokio::test]
async fn denied_preparation_prevents_lua_module_evaluation_and_model_use() {
    let temp = TempDir::new().unwrap();
    let script = temp.path().join("echo.lua");
    fs::write(&script, "error('MODULE_EVALUATED')").unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let cfg = Arc::new(config(temp.path()));
    let runtime = AgentRuntime::new((*cfg).clone(), temp.path(), ModelRegistry::default())
        .await
        .unwrap()
        .with_policy(
            policy(),
            Arc::new(context_harness::agent_runtime::policy::DenyApprovals),
        )
        .with_tool_binding_catalog(&directories, &catalog(&script, cfg), authority(temp.path()))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime.run(&agent(), "Echo", cancel).await.unwrap();
    assert_eq!(run.status, "failed");
    assert!(run.error.as_deref().unwrap().contains("approval denied"));
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].tool_name.starts_with("runtime.lua.prepare."));
    assert_eq!(calls[0].status, "denied");
    let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
    assert!(!events
        .iter()
        .any(|event| event.event_type == "model.requested"));
}

#[tokio::test]
async fn approved_lua_binding_prepares_then_executes_with_effective_arguments() {
    let temp = TempDir::new().unwrap();
    let script = temp.path().join("echo.lua");
    fs::write(
        &script,
        r#"
tool = {}
function tool.execute(params, context)
  return { value = params.prefix .. params.text }
end
"#,
    )
    .unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let cfg = Arc::new(config(temp.path()));
    let provider = Arc::new(FakeModel::new([
        Ok(tool_call(json!({"text":"hello"}))),
        Ok(ModelResponse::text("done")),
    ]));
    let mut models = ModelRegistry::default();
    models.register("test", "fake", "lua", provider).unwrap();
    let approvals = Arc::new(Grant {
        count: AtomicUsize::new(0),
        arguments: Mutex::new(Vec::new()),
    });
    let runtime = AgentRuntime::new((*cfg).clone(), temp.path(), models)
        .await
        .unwrap()
        .with_policy(policy(), approvals.clone())
        .with_tool_binding_catalog(&directories, &catalog(&script, cfg), authority(temp.path()))
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime.run(&agent(), "Echo", cancel).await.unwrap();
    assert_eq!(run.status, "completed");
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].tool_name.starts_with("runtime.lua.prepare."));
    assert_eq!(calls[1].tool_name, "fixture.lua_echo");
    assert!(calls[1]
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("lua: hello"));
    assert_eq!(approvals.count.load(Ordering::SeqCst), 2);
    assert_eq!(approvals.arguments.lock().unwrap()[1]["prefix"], "lua: ");
    let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
    let resolved = events
        .iter()
        .find(|event| event.event_type == "context.resolved")
        .unwrap();
    let binding = &resolved.payload["tool_bindings"]["fixture.lua_echo"];
    assert_eq!(binding["implementation_id"], "lua.fixture.echo");
    assert_eq!(binding["fixed"]["prefix"], "lua: ");
    assert_eq!(binding["restrictions"]["max_output_bytes"], 256);
}

#[tokio::test]
async fn lua_binding_validation_failure_execution_failure_and_output_bounds_are_durable() {
    for (arguments, expected_status) in [
        (json!({"unknown":"value"}), "denied"),
        (json!({"text":"fail"}), "failed"),
        (json!({"text":"large"}), "failed"),
    ] {
        let temp = TempDir::new().unwrap();
        let script = temp.path().join("echo.lua");
        fs::write(
            &script,
            r#"
tool = {}
function tool.execute(params, context)
  if params.text == "fail" then error("fixture failure") end
  if params.text == "large" then return { value = string.rep("x", 512) } end
  return { value = params.prefix .. params.text }
end
"#,
        )
        .unwrap();
        let tools = temp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
        let directories = [ResourceDirectory {
            path: tools,
            scope: ResourceScope::Workspace,
        }];
        let cfg = Arc::new(config(temp.path()));
        let mut models = ModelRegistry::default();
        models
            .register(
                "test",
                "fake",
                "lua",
                Arc::new(FakeModel::new([Ok(tool_call(arguments))])),
            )
            .unwrap();
        let approvals = Arc::new(Grant {
            count: AtomicUsize::new(0),
            arguments: Mutex::new(Vec::new()),
        });
        let runtime = AgentRuntime::new((*cfg).clone(), temp.path(), models)
            .await
            .unwrap()
            .with_policy(policy(), approvals)
            .with_tool_binding_catalog(&directories, &catalog(&script, cfg), authority(temp.path()))
            .await
            .unwrap();
        let (_sender, cancel) = watch::channel(false);
        let run = runtime.run(&agent(), "Echo", cancel).await.unwrap();
        assert_eq!(run.status, "failed");
        let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].status, "completed");
        assert_eq!(calls[1].status, expected_status);
    }
}

#[tokio::test]
async fn cancelling_cpu_bound_lua_finalizes_the_invocation() {
    let temp = TempDir::new().unwrap();
    let script = temp.path().join("echo.lua");
    fs::write(
        &script,
        r#"
tool = {}
function tool.execute(params, context)
  while true do end
end
"#,
    )
    .unwrap();
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("echo.toml"), RESOURCE).unwrap();
    let directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let cfg = Arc::new(config(temp.path()));
    let mut models = ModelRegistry::default();
    models
        .register(
            "test",
            "fake",
            "lua",
            Arc::new(FakeModel::new([Ok(tool_call(json!({"text":"loop"})))])),
        )
        .unwrap();
    let approvals = Arc::new(Grant {
        count: AtomicUsize::new(0),
        arguments: Mutex::new(Vec::new()),
    });
    let runtime = Arc::new(
        AgentRuntime::new((*cfg).clone(), temp.path(), models)
            .await
            .unwrap()
            .with_policy(policy(), approvals)
            .with_tool_binding_catalog(&directories, &catalog(&script, cfg), authority(temp.path()))
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
                if calls.get(1).is_some_and(|call| call.status == "started") {
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
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].status, "failed");
    assert_eq!(calls[1].error.as_deref(), Some("run interrupted"));
}
