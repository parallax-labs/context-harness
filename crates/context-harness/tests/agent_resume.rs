use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, *},
    agent_resource::{
        AgentResource, Capability, LoadedAgentResource, ModelDefinition, ResourceScope,
    },
    agent_runtime::{
        policy::{DenyApprovals, RuntimePolicy},
        AgentRuntime,
    },
    config::Config,
};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::watch;

fn config(tmp: &TempDir) -> Config {
    let mut c = Config::minimal();
    c.db.path = tmp.path().join(".ctx/data/ctx.sqlite");
    c.models.insert(
        "test".into(),
        ModelDefinition {
            provider: "fake".into(),
            model: "test".into(),
            api_key_env: None,
            ..Default::default()
        },
    );
    c
}
fn resource(tools: &[&str]) -> LoadedAgentResource {
    let definition = AgentResource::parse(&format!(
        "[agent]\nname='researcher'\nmodel='test'\ntools={}\n[prompt]\nsystem='Use context.'\n",
        serde_json::to_string(tools).unwrap()
    ))
    .unwrap();
    LoadedAgentResource {
        path: "test.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    }
}
async fn runtime(tmp: &TempDir, provider: Arc<dyn ModelProvider>) -> AgentRuntime {
    let mut registry = ModelRegistry::default();
    registry.register("test", "fake", "test", provider).unwrap();
    AgentRuntime::new(config(tmp), tmp.path(), registry)
        .await
        .unwrap()
}
struct Block;
#[async_trait]
impl ModelProvider for Block {
    async fn generate(&self, _: &ModelRequest) -> ModelResult<ModelResponse> {
        std::future::pending().await
    }
}
struct ReadThenBlock(AtomicUsize);
#[async_trait]
impl ModelProvider for ReadThenBlock {
    async fn generate(&self, _: &ModelRequest) -> ModelResult<ModelResponse> {
        if self.0.fetch_add(1, Ordering::SeqCst) > 0 {
            std::future::pending().await
        }
        let mut r = ModelResponse::text("");
        r.finish_reason = FinishReason::ToolCalls;
        r.tool_calls = vec![ToolCall {
            id: "read".into(),
            name: "workspace.read".into(),
            arguments: json!({"path":"a.txt"}),
        }];
        Ok(r)
    }
}
struct VerifyConversation;
#[async_trait]
impl ModelProvider for VerifyConversation {
    async fn generate(&self, r: &ModelRequest) -> ModelResult<ModelResponse> {
        assert_eq!(r.messages.len(), 4);
        let ModelMessage::Tool { call_id, content } = r.messages.last().unwrap() else {
            panic!("missing completed tool result")
        };
        assert_eq!(call_id, "read");
        assert!(content.contains("original contents"));
        Ok(ModelResponse::text("recovered"))
    }
}
async fn wait_event(runtime: &AgentRuntime, kind: &str, count: usize) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(run) = runtime.store().history(1).await.unwrap().first() {
                if runtime
                    .store()
                    .events(&run.id, 0, 1000)
                    .await
                    .unwrap()
                    .iter()
                    .filter(|e| e.event_type == kind)
                    .count()
                    >= count
                {
                    return run.id.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
async fn crash(tmp: &TempDir, res: LoadedAgentResource) -> (Arc<AgentRuntime>, String) {
    let runtime = Arc::new(runtime(tmp, Arc::new(Block)).await);
    let rt = runtime.clone();
    let (_tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move { rt.run(&res, "question", rx).await });
    let id = wait_event(&runtime, "model.requested", 1).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    (runtime, id)
}
#[tokio::test]
async fn crash_after_completed_turn_restores_conversation_without_replaying_tools() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "original contents").unwrap();
    let rt = Arc::new(runtime(&tmp, Arc::new(ReadThenBlock(AtomicUsize::new(0)))).await);
    let res = resource(&["workspace.read"]);
    let copy = res.clone();
    let rt2 = rt.clone();
    let (_tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move { rt2.run(&copy, "question", rx).await });
    let id = wait_event(&rt, "model.requested", 2).await;
    // A second owner is rejected before mutating history.
    let competing = runtime(&tmp, Arc::new(VerifyConversation)).await;
    let (_tx, rx) = watch::channel(false);
    assert!(competing
        .resume(&id, &res, rx)
        .await
        .unwrap_err()
        .to_string()
        .contains("active owner"));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    std::fs::remove_file(tmp.path().join("a.txt")).unwrap();
    let (_tx, rx) = watch::channel(false);
    let resumed = competing.resume(&id, &res, rx).await.unwrap();
    assert_eq!(resumed.status, "completed");
    assert_eq!(resumed.output.as_deref(), Some("recovered"));
    assert_eq!(
        competing.store().tool_invocations(&id).await.unwrap().len(),
        1
    );
    let checkpoint = competing
        .store()
        .latest_checkpoint(&id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.turn, 2);
}
#[tokio::test]
async fn rejects_changed_bindings_checkpoint_schema_and_exhausted_budget() {
    let tmp = TempDir::new().unwrap();
    let res = resource(&[]);
    let (rt, id) = crash(&tmp, res.clone()).await;
    let before = rt
        .store()
        .get_run(&id)
        .await
        .unwrap()
        .unwrap()
        .last_sequence;
    let mut changed = res.clone();
    changed.definition.prompt.system = "different".into();
    changed.version = changed.definition.version().unwrap();
    let (_tx, rx) = watch::channel(false);
    assert!(rt.resume(&id, &changed, rx).await.is_err());
    assert_eq!(
        rt.store()
            .get_run(&id)
            .await
            .unwrap()
            .unwrap()
            .last_sequence,
        before
    );
    let altered = runtime(&tmp, Arc::new(Block)).await.with_policy(
        RuntimePolicy {
            allow: vec![],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let (_tx, rx) = watch::channel(false);
    assert!(altered
        .resume(&id, &res, rx)
        .await
        .unwrap_err()
        .to_string()
        .contains("binding"));
    rt.store()
        .save_checkpoint(&id, 99, 0, &json!({}))
        .await
        .unwrap();
    let (_tx, rx) = watch::channel(false);
    assert!(rt
        .resume(&id, &res, rx)
        .await
        .unwrap_err()
        .to_string()
        .contains("version"));
    let tmp = TempDir::new().unwrap();
    let mut res = resource(&[]);
    res.definition.agent.execution.max_turns = 1;
    res.version = res.definition.version().unwrap();
    let (rt, id) = crash(&tmp, res.clone()).await;
    let (_tx, rx) = watch::channel(false);
    assert!(rt
        .resume(&id, &res, rx)
        .await
        .unwrap_err()
        .to_string()
        .contains("budget"));
}
#[tokio::test]
async fn cancelled_model_call_can_resume_but_original_deadline_is_retained() {
    let tmp = TempDir::new().unwrap();
    let rt = Arc::new(runtime(&tmp, Arc::new(Block)).await);
    let mut res = resource(&[]);
    res.definition.agent.execution.timeout_seconds = 1;
    res.version = res.definition.version().unwrap();
    let copy = res.clone();
    let rt2 = rt.clone();
    let (tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move { rt2.run(&copy, "question", rx).await });
    let id = wait_event(&rt, "model.requested", 1).await;
    tx.send(true).unwrap();
    assert_eq!(task.await.unwrap().unwrap().status, "cancelled");
    tokio::time::sleep(Duration::from_millis(1050)).await;
    let (_tx, rx) = watch::channel(false);
    assert!(rt
        .resume(&id, &res, rx)
        .await
        .unwrap_err()
        .to_string()
        .contains("deadline"));
}
#[cfg(unix)]
#[tokio::test]
async fn crash_during_side_effect_is_never_replayed() {
    let tmp = TempDir::new().unwrap();
    let mut res = resource(&["process.exec"]);
    res.definition.agent.permissions.allow = Some(vec![Capability::ProcessExecute]);
    res.version = res.definition.version().unwrap();
    let mut response = ModelResponse::text("");
    response.finish_reason = FinishReason::ToolCalls;
    response.tool_calls = vec![ToolCall {
        id: "exec".into(),
        name: "process.exec".into(),
        arguments: json!({"argv":["/bin/sh","-c","printf once >> marker; exec sleep 30"]}),
    }];
    let rt = Arc::new(
        runtime(&tmp, Arc::new(FakeModel::new([Ok(response)])))
            .await
            .with_policy(
                RuntimePolicy {
                    allow: vec![Capability::ProcessExecute],
                    require_approval: vec![],
                },
                Arc::new(DenyApprovals),
            ),
    );
    let copy = res.clone();
    let rt2 = rt.clone();
    let (_tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move { rt2.run(&copy, "question", rx).await });
    let id = wait_event(&rt, "tool.started", 1).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while std::fs::read_to_string(tmp.path().join("marker")).unwrap_or_default() != "once" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let (_tx, rx) = watch::channel(false);
    assert!(rt
        .resume(&id, &res, rx)
        .await
        .unwrap_err()
        .to_string()
        .contains("reconciliation"));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("marker")).unwrap(),
        "once"
    );
}
#[tokio::test]
async fn large_final_output_is_a_hashed_artifact_and_completed_runs_do_not_resume() {
    use sha2::{Digest, Sha256};
    let tmp = TempDir::new().unwrap();
    let text = "x".repeat(70000);
    let rt = runtime(
        &tmp,
        Arc::new(FakeModel::new([Ok(ModelResponse::text(&text))])),
    )
    .await;
    let res = resource(&[]);
    let (_tx, rx) = watch::channel(false);
    let run = rt.run(&res, "question", rx).await.unwrap();
    assert_eq!(run.status, "completed");
    assert!(run.output.as_ref().unwrap().len() < 1000);
    let artifacts = rt.store().artifacts(&run.id).await.unwrap();
    assert_eq!(artifacts.len(), 1);
    let artifact = &artifacts[0];
    assert_eq!(
        std::fs::read(tmp.path().join(&artifact.relative_path)).unwrap(),
        text.as_bytes()
    );
    assert_eq!(
        artifact.sha256,
        format!("{:x}", Sha256::digest(text.as_bytes()))
    );
    assert_eq!(artifact.size, 70000);
    let (_tx, rx) = watch::channel(false);
    assert!(rt.resume(&run.id, &res, rx).await.is_err());
}
#[tokio::test]
async fn cli_resumes_cancelled_run_and_inspects_checkpoint_history() {
    let tmp = TempDir::new().unwrap();
    let rt = Arc::new(runtime(&tmp, Arc::new(Block)).await);
    let res = resource(&[]);
    let copy = res.clone();
    let rt2 = rt.clone();
    let (tx, rx) = watch::channel(false);
    let task = tokio::spawn(async move { rt2.run(&copy, "question", rx).await });
    let id = wait_event(&rt, "model.requested", 1).await;
    tx.send(true).unwrap();
    assert_eq!(task.await.unwrap().unwrap().status, "cancelled");
    let cfg = tmp.path().join(".ctx/config.toml");
    std::fs::write(&cfg,"[db]\npath='.ctx/data/ctx.sqlite'\n[chunking]\nmax_tokens=700\n[retrieval]\nfinal_limit=12\n[server]\nbind='127.0.0.1:0'\n[models.test]\nprovider='fake'\nmodel='test'\n").unwrap();
    std::fs::create_dir_all(tmp.path().join(".ctx/agents")).unwrap();
    std::fs::write(
        tmp.path().join(".ctx/agents/researcher.toml"),
        toml::to_string(&res.definition).unwrap(),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(tmp.path())
        .arg("--config")
        .arg(&cfg)
        .args(["agent", "resume", &id, "--json", "--non-interactive"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["id"], id);
    assert_eq!(value["status"], "completed");
}

#[cfg(unix)]
#[tokio::test]
async fn unsafe_ownership_path_returns_durable_failure() {
    let tmp = TempDir::new().unwrap();
    let rt = runtime(&tmp, Arc::new(Block)).await;
    let outside = TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), tmp.path().join(".ctx/runs")).unwrap();
    let (_tx, rx) = watch::channel(false);
    let run = rt.run(&resource(&[]), "question", rx).await.unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(run.error.as_deref(), Some("run ownership is unavailable"));
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
