use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, *},
    agent_resource::{AgentResource, Capability, LoadedAgentResource, ResourceScope},
    agent_runtime::{policy::*, AgentRuntime},
    config::{Config, McpServerConfig},
};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::watch;

fn config(root: &Path, mode: &str) -> Config {
    let mut cfg = Config::minimal();
    cfg.db.path = root.join("ctx.sqlite");
    cfg.mcp_servers.insert(
        "fixture".into(),
        McpServerConfig {
            command: "python3".into(),
            args: vec![
                format!(
                    "{}/tests/fixtures/runtime_mcp_server.py",
                    env!("CARGO_MANIFEST_DIR")
                ),
                mode.into(),
            ],
            timeout_seconds: 1,
        },
    );
    cfg
}
fn resource() -> LoadedAgentResource {
    let definition=AgentResource::parse("[agent]\nname='external'\nmodel='test'\ntools=['mcp.fixture.echo']\n[agent.permissions]\nallow=['read_only']\nrequire_approval=['process_execute','external_side_effect']\n[prompt]\nsystem='Use the external tool.'").unwrap();
    LoadedAgentResource {
        path: "test.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    }
}
fn call() -> ModelResponse {
    let mut response = ModelResponse::text("");
    response.finish_reason = FinishReason::ToolCalls;
    response.tool_calls = vec![ToolCall {
        id: "echo1".into(),
        name: "mcp.fixture.echo".into(),
        arguments: json!({"text":"hello"}),
    }];
    response
}
fn models(provider: Arc<dyn ModelProvider>) -> ModelRegistry {
    let mut registry = ModelRegistry::default();
    registry.register("test", "fake", "test", provider).unwrap();
    registry
}
struct Decide {
    deny_call: bool,
    calls: Mutex<Vec<String>>,
}
#[async_trait]
impl ApprovalHandler for Decide {
    async fn approve(&self, request: &ApprovalRequest) -> bool {
        assert!(request.capabilities.contains(&Capability::ProcessExecute));
        assert!(request
            .capabilities
            .contains(&Capability::ExternalSideEffect));
        self.calls.lock().unwrap().push(request.tool.clone());
        !(self.deny_call && request.tool.starts_with("mcp."))
    }
}
struct Verify;
#[async_trait]
impl ModelProvider for Verify {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse> {
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "mcp.fixture.echo");
        assert_eq!(request.tools[0].parameters["type"], "object");
        if request.messages.len() == 2 {
            return Ok(call());
        }
        let ModelMessage::Tool { content, .. } = request.messages.last().unwrap() else {
            panic!("missing external result")
        };
        assert!(content.contains("external: hello"));
        Ok(ModelResponse::text("done"))
    }
}
async fn execute(
    tmp: &TempDir,
    mode: &str,
    deny_call: bool,
) -> (
    AgentRuntime,
    context_harness::agent_store::AgentRun,
    Arc<Decide>,
) {
    let handler = Arc::new(Decide {
        deny_call,
        calls: Mutex::new(vec![]),
    });
    let rt = AgentRuntime::new(
        config(tmp.path(), mode),
        tmp.path(),
        models(Arc::new(Verify)),
    )
    .await
    .unwrap()
    .with_policy(RuntimePolicy::default(), handler.clone());
    let (_tx, rx) = watch::channel(false);
    let run = rt.run(&resource(), "question", rx).await.unwrap();
    (rt, run, handler)
}
#[cfg(unix)]
async fn assert_stopped(root: &Path) {
    let pid: i32 = std::fs::read_to_string(root.join("mcp-pid"))
        .unwrap()
        .parse()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if unsafe { libc::kill(pid, 0) } == -1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("MCP child still alive");
}
#[tokio::test]
async fn discovered_tool_uses_normal_policy_history_and_namespaces() {
    let tmp = TempDir::new().unwrap();
    let (rt, run, handler) = execute(&tmp, "paged", false).await;
    assert_eq!(run.status, "completed", "{:?}", run.error);
    assert_eq!(run.output.as_deref(), Some("done"));
    assert_eq!(
        *handler.calls.lock().unwrap(),
        vec!["runtime.mcp.start.fixture", "mcp.fixture.echo"]
    );
    let calls = rt.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.status == "completed"));
    assert!(rt
        .store()
        .latest_checkpoint(&run.id)
        .await
        .unwrap()
        .is_none());
    let (_tx, rx) = watch::channel(false);
    assert!(rt.resume(&run.id, &resource(), rx).await.is_err());
    #[cfg(unix)]
    assert_stopped(tmp.path()).await;
}
#[tokio::test]
async fn policy_denial_and_noninteractive_approval_do_not_spawn_server() {
    for read_only in [false, true] {
        let tmp = TempDir::new().unwrap();
        let rt = AgentRuntime::new(
            config(tmp.path(), "ok"),
            tmp.path(),
            models(Arc::new(Verify)),
        )
        .await
        .unwrap();
        let mut resource = resource();
        if read_only {
            resource.definition.agent.permissions = Default::default();
        }
        let (_tx, rx) = watch::channel(false);
        let run = rt.run(&resource, "question", rx).await.unwrap();
        assert_eq!(run.status, "failed");
        assert!(!tmp.path().join("mcp-pid").exists());
        let events = rt.store().events(&run.id, 0, 100).await.unwrap();
        assert!(!events.iter().any(|e| e.event_type == "model.requested"));
        assert_eq!(
            events.iter().any(|e| e.event_type == "approval.denied"),
            !read_only
        );
    }
}
#[tokio::test]
async fn remote_readonly_hint_cannot_bypass_call_approval() {
    let tmp = TempDir::new().unwrap();
    let (rt, run, handler) = execute(&tmp, "ok", true).await;
    assert_eq!(run.status, "failed");
    assert_eq!(handler.calls.lock().unwrap().len(), 2);
    let messages = std::fs::read_to_string(tmp.path().join("mcp-messages")).unwrap();
    assert!(!messages.contains("tools/call"));
    assert_eq!(
        rt.store().tool_invocations(&run.id).await.unwrap()[1].status,
        "denied"
    );
    #[cfg(unix)]
    assert_stopped(tmp.path()).await;
}
#[tokio::test]
async fn malformed_discovery_timeouts_disconnect_and_errors_fail_closed() {
    for mode in [
        "startup-timeout",
        "oversized",
        "malformed",
        "duplicate",
        "many",
        "cycle",
        "call-timeout",
        "disconnect",
        "call-error",
    ] {
        let tmp = TempDir::new().unwrap();
        let (rt, run, _) = execute(&tmp, mode, false).await;
        assert_eq!(run.status, "failed", "mode={mode}");
        assert!(!run
            .error
            .as_ref()
            .unwrap()
            .contains("fixture-private-error"));
        assert!(rt
            .store()
            .tool_invocations(&run.id)
            .await
            .unwrap()
            .iter()
            .any(|c| c.status == "failed"));
        #[cfg(unix)]
        assert_stopped(tmp.path()).await;
    }
}
#[tokio::test]
async fn explicit_host_permission_can_run_without_approval_and_unused_servers_stay_idle() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = config(tmp.path(), "ok");
    cfg.mcp_servers.insert(
        "unused".into(),
        McpServerConfig {
            command: "never-run-this".into(),
            args: vec![],
            timeout_seconds: 1,
        },
    );
    let mut res = resource();
    res.definition.agent.permissions.allow = Some(vec![
        Capability::ProcessExecute,
        Capability::ExternalSideEffect,
    ]);
    res.definition.agent.permissions.require_approval.clear();
    let rt = AgentRuntime::new(cfg, tmp.path(), models(Arc::new(Verify)))
        .await
        .unwrap()
        .with_policy(
            RuntimePolicy {
                allow: vec![Capability::ProcessExecute, Capability::ExternalSideEffect],
                require_approval: vec![],
            },
            Arc::new(DenyApprovals),
        );
    let (_tx, rx) = watch::channel(false);
    let run = rt.run(&res, "question", rx).await.unwrap();
    assert_eq!(run.status, "completed");
}
#[tokio::test]
async fn cancellation_during_server_call_stops_child_and_finalizes_invocation() {
    let tmp = TempDir::new().unwrap();
    let handler = Arc::new(Decide {
        deny_call: false,
        calls: Mutex::new(vec![]),
    });
    let rt = Arc::new(
        AgentRuntime::new(
            config(tmp.path(), "call-timeout"),
            tmp.path(),
            models(Arc::new(FakeModel::new([Ok(call())]))),
        )
        .await
        .unwrap()
        .with_policy(RuntimePolicy::default(), handler),
    );
    let rt2 = rt.clone();
    let (tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move { rt2.run(&resource(), "question", rx).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(tmp.path().join("mcp-messages"))
                .unwrap_or_default()
                .contains("tools/call")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tx.send(true).unwrap();
    let run = task.await.unwrap().unwrap();
    assert_eq!(run.status, "cancelled");
    let calls = rt.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls[1].status, "failed");
    #[cfg(unix)]
    assert_stopped(tmp.path()).await;
}

#[tokio::test]
async fn server_requests_are_denied_without_exposing_workspace_roots() {
    let tmp = TempDir::new().unwrap();
    let (_rt, run, _) = execute(&tmp, "callback", false).await;
    assert_eq!(run.status, "completed");
    let lines = std::fs::read_to_string(tmp.path().join("mcp-messages")).unwrap();
    let messages: Vec<serde_json::Value> = lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let answer = messages
        .iter()
        .find(|msg| msg["id"] == "server-request")
        .expect("missing server request rejection");
    assert!(answer.get("error").is_some());
    assert!(answer.get("result").is_none());
}

#[test]
fn cli_noninteractive_denies_startup_before_spawning() {
    use context_harness::agent_resource::ModelDefinition;
    let tmp = TempDir::new().unwrap();
    let mut cfg = config(tmp.path(), "ok");
    cfg.models.insert(
        "test".into(),
        ModelDefinition {
            provider: "fake".into(),
            model: "test".into(),
            api_key_env: None,
        },
    );
    let config_path = tmp.path().join("config.toml");
    std::fs::write(&config_path, toml::to_string(&json!({"db":{"path":"ctx.sqlite"},"chunking":{"max_tokens":700},"retrieval":{"final_limit":12},"server":{"bind":"127.0.0.1:0"},"models":cfg.models,"mcp_servers":cfg.mcp_servers})).unwrap()).unwrap();
    std::fs::create_dir(tmp.path().join("agents")).unwrap();
    std::fs::write(
        tmp.path().join("agents/external.toml"),
        toml::to_string(&resource().definition).unwrap(),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(tmp.path())
        .arg("--config")
        .arg(config_path)
        .args([
            "agent",
            "run",
            "external",
            "question",
            "--json",
            "--non-interactive",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("durable run JSON");
    assert_eq!(value["status"], "failed");
    assert_eq!(value["error"], "tool approval denied");
    assert!(!tmp.path().join("mcp-pid").exists());
}
