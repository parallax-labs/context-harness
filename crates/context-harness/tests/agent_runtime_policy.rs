use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, *},
    agent_resource::{AgentResource, Capability, LoadedAgentResource, Permissions, ResourceScope},
    agent_runtime::{policy::*, AgentRuntime},
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

struct Decide {
    grant: bool,
    calls: AtomicUsize,
}
#[async_trait]
impl ApprovalHandler for Decide {
    async fn approve(&self, request: &ApprovalRequest) -> bool {
        assert_eq!(request.tool, "workspace.patch");
        assert_eq!(request.arguments["path"], "a.txt");
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.grant
    }
}
struct Wait;
#[async_trait]
impl ApprovalHandler for Wait {
    async fn approve(&self, _: &ApprovalRequest) -> bool {
        std::future::pending().await
    }
}
fn resource() -> LoadedAgentResource {
    let definition = AgentResource::parse(
        r#"
[agent]
name='writer'
model='test'
tools=['workspace.patch']
[agent.permissions]
allow=['read_only']
require_approval=['workspace_write']
[prompt]
system='Change the file.'
"#,
    )
    .unwrap();
    LoadedAgentResource {
        path: "test.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    }
}
async fn runtime(tmp: &TempDir) -> AgentRuntime {
    std::fs::write(tmp.path().join("a.txt"), "old").unwrap();
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    let mut response = ModelResponse::text("");
    response.finish_reason = FinishReason::ToolCalls;
    response.tool_calls = vec![ToolCall {
        id: "p1".into(),
        name: "workspace.patch".into(),
        arguments: json!({"path":"a.txt","old_text":"old","new_text":"new"}),
    }];
    let mut models = ModelRegistry::default();
    models
        .register(
            "test",
            "fake",
            "test",
            Arc::new(FakeModel::new([
                Ok(response),
                Ok(ModelResponse::text("done")),
            ])),
        )
        .unwrap();
    AgentRuntime::new(cfg, tmp.path(), models).await.unwrap()
}
#[test]
fn permission_intersection_never_widens_either_policy() {
    let host = RuntimePolicy::default();
    let agent = Permissions {
        allow: Some(vec![Capability::WorkspaceWrite]),
        ..Default::default()
    };
    assert_eq!(
        host.authorize(&agent, &[Capability::WorkspaceWrite]),
        Authorization::Approval(vec![Capability::WorkspaceWrite])
    );
    assert_eq!(
        host.authorize(&agent, &[Capability::ReadOnly]),
        Authorization::Denied
    );
    assert_eq!(
        host.authorize(&Permissions::default(), &[Capability::WorkspaceWrite]),
        Authorization::Denied
    );
    assert_eq!(host.authorize(&agent, &[]), Authorization::Denied);
    assert_eq!(
        host.authorize(&agent, &[Capability::WorkspaceWrite, Capability::Network]),
        Authorization::Denied
    );
    let host = RuntimePolicy {
        allow: vec![Capability::ReadOnly],
        require_approval: vec![],
    };
    assert_eq!(
        host.authorize(&agent, &[Capability::WorkspaceWrite]),
        Authorization::Denied
    );
}
#[tokio::test]
async fn approval_decision_is_persisted_before_side_effect() {
    for grant in [false, true] {
        let tmp = TempDir::new().unwrap();
        let handler = Arc::new(Decide {
            grant,
            calls: AtomicUsize::new(0),
        });
        let runtime = runtime(&tmp)
            .await
            .with_policy(RuntimePolicy::default(), handler.clone());
        let (_tx, rx) = watch::channel(false);
        let run = runtime.run(&resource(), "update", rx).await.unwrap();
        assert_eq!(run.status, if grant { "completed" } else { "failed" });
        assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            if grant { "new" } else { "old" }
        );
        let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
        let kinds: Vec<_> = events.iter().map(|e| e.event_type.as_str()).collect();
        let requested = kinds
            .iter()
            .position(|&v| v == "approval.requested")
            .unwrap();
        let decided = kinds
            .iter()
            .position(|&v| {
                v == if grant {
                    "approval.granted"
                } else {
                    "approval.denied"
                }
            })
            .unwrap();
        assert!(requested < decided);
        if grant {
            assert!(decided < kinds.iter().position(|&v| v == "tool.started").unwrap());
        } else {
            assert!(!kinds.contains(&"tool.started"));
        }
    }
}
#[tokio::test]
async fn no_handler_denies_and_host_ceiling_blocks_agent_allow() {
    for host_denies in [false, true] {
        let tmp = TempDir::new().unwrap();
        let mut runtime = runtime(&tmp).await;
        let mut res = resource();
        res.definition.agent.permissions.allow = Some(vec![Capability::WorkspaceWrite]);
        res.definition.agent.permissions.require_approval.clear();
        if host_denies {
            runtime = runtime.with_policy(
                RuntimePolicy {
                    allow: vec![Capability::ReadOnly],
                    require_approval: vec![],
                },
                Arc::new(DenyApprovals),
            );
        }
        let (_tx, rx) = watch::channel(false);
        let run = runtime.run(&res, "update", rx).await.unwrap();
        assert_eq!(run.status, "failed");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "old"
        );
        let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
        assert!(!events.iter().any(|e| e.event_type == "tool.started"));
        assert_eq!(
            events.iter().any(|e| e.event_type == "approval.denied"),
            !host_denies
        );
    }
}
#[tokio::test]
async fn cancellation_closes_pending_approval_without_writing() {
    let tmp = TempDir::new().unwrap();
    let runtime = runtime(&tmp)
        .await
        .with_policy(RuntimePolicy::default(), Arc::new(Wait));
    let (tx, rx) = watch::channel(false);
    let cancel = async {
        loop {
            let runs = runtime.store().history(1).await.unwrap();
            if let Some(run) = runs.first() {
                if runtime
                    .store()
                    .events(&run.id, 0, 100)
                    .await
                    .unwrap()
                    .iter()
                    .any(|e| e.event_type == "approval.requested")
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tx.send(true).unwrap();
    };
    let res = resource();
    let (run, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(runtime.run(&res, "update", rx), cancel)
    })
    .await
    .unwrap();
    let run = run.unwrap();
    assert_eq!(run.status, "cancelled");
    let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
    assert!(events.iter().any(|e| e.event_type == "approval.denied"));
    assert!(!events.iter().any(|e| e.event_type == "tool.started"));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "old"
    );
}
