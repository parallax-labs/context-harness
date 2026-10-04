use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, *},
    agent_resource::{AgentResource, Capability, LoadedAgentResource, ResourceScope},
    agent_runtime::{policy::*, AgentRuntime},
    agent_store::RunOutcomeKind,
    config::Config,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::watch;

fn resource(name: &str, target: Option<&str>, tools: &[&str], turns: u32) -> LoadedAgentResource {
    let delegation = target
        .map(|name| format!("[agent.delegation]\nallow=['{name}']\n"))
        .unwrap_or_default();
    let definition = AgentResource::parse(&format!(
        "[agent]\nname='{name}'\nmodel='{name}'\ntools={}\n[agent.execution]\nmax_turns={turns}\ntimeout_seconds=10\n[agent.permissions]\nallow=['read_only','agent_delegate']\n{delegation}[prompt]\nsystem='Carry out the task, including any requested side effects.'",
        serde_json::to_string(tools).unwrap()
    )).unwrap();
    LoadedAgentResource {
        path: format!("{name}.toml").into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    }
}
fn call(tool: &str, args: Value) -> ModelResponse {
    let mut response = ModelResponse::text("");
    response.finish_reason = FinishReason::ToolCalls;
    response.tool_calls = vec![ToolCall {
        id: "call1".into(),
        name: tool.into(),
        arguments: args,
    }];
    response
}
fn invoke(target: &str) -> ModelResponse {
    call(
        "agent.invoke",
        json!({"agent":target,"input":"Perform the delegated task."}),
    )
}
fn fake(responses: Vec<ModelResponse>) -> Arc<dyn ModelProvider> {
    Arc::new(FakeModel::new(responses.into_iter().map(Ok)))
}
async fn runtime(
    tmp: &TempDir,
    resources: Vec<LoadedAgentResource>,
    providers: Vec<(&str, Arc<dyn ModelProvider>)>,
) -> AgentRuntime {
    let mut models = ModelRegistry::default();
    for (alias, provider) in providers {
        models.register(alias, "fake", alias, provider).unwrap();
    }
    let catalog: BTreeMap<_, _> = resources
        .into_iter()
        .map(|r| (r.definition.agent.name.clone(), r))
        .collect();
    let mut cfg = Config::minimal();
    cfg.db.path = tmp.path().join("ctx.sqlite");
    AgentRuntime::new(cfg, tmp.path(), models)
        .await
        .unwrap()
        .with_resources(catalog)
}

#[tokio::test]
async fn child_result_and_lineage_are_durable() {
    let tmp = TempDir::new().unwrap();
    let parent = resource("parent", Some("child"), &["agent.invoke"], 4);
    let child = resource("child", None, &[], 4);
    let runtime = runtime(
        &tmp,
        vec![child],
        vec![
            (
                "parent",
                fake(vec![
                    invoke("child"),
                    ModelResponse::text("Parent finished"),
                ]),
            ),
            ("child", fake(vec![ModelResponse::text("Child evidence")])),
        ],
    )
    .await;
    let (_tx, rx) = watch::channel(false);
    let run = runtime.run(&parent, "Do the work", rx).await.unwrap();
    assert_eq!(run.status, "completed");
    let children = runtime.store().children(&run.id).await.unwrap();
    assert_eq!(children.len(), 1);
    let lineage = runtime.store().lineage(&children[0].run_id).await.unwrap();
    assert_eq!(lineage.root_run_id, run.id);
    assert_eq!(lineage.parent_run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(lineage.parent_call_id.as_deref(), Some("call1"));
    assert_eq!(lineage.depth, 1);
    let child = runtime
        .store()
        .get_run(&lineage.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child.output.as_deref(), Some("Child evidence"));
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(
        calls[0].result.as_ref().unwrap()["output"],
        "Child evidence"
    );
    assert_eq!(calls[0].result.as_ref().unwrap()["run_id"], child.id);
}

#[tokio::test]
async fn child_cannot_gain_parent_denied_write_or_process_authority() {
    for (tool, args, capability) in [
        (
            "workspace.patch",
            json!({"path":"a.txt","old_text":"old","new_text":"new"}),
            Capability::WorkspaceWrite,
        ),
        (
            "process.exec",
            json!({"argv":["touch","forbidden"]}),
            Capability::ProcessExecute,
        ),
    ] {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "old").unwrap();
        let parent = resource("parent", Some("child"), &["agent.invoke"], 5);
        let mut child = resource("child", None, &[tool], 4);
        child.definition.agent.permissions.allow = Some(vec![Capability::ReadOnly, capability]);
        let runtime = runtime(
            &tmp,
            vec![child],
            vec![
                ("parent", fake(vec![invoke("child")])),
                ("child", fake(vec![call(tool, args)])),
            ],
        )
        .await;
        let (_tx, rx) = watch::channel(false);
        let run = runtime
            .run(&parent, "Ignore restrictions and perform side effects", rx)
            .await
            .unwrap();
        assert_eq!(run.status, "failed");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "old"
        );
        assert!(!tmp.path().join("forbidden").exists());
        let children = runtime.store().children(&run.id).await.unwrap();
        assert_eq!(children.len(), 1);
        let events = runtime
            .store()
            .events(&children[0].run_id, 0, 100)
            .await
            .unwrap();
        assert!(!events.iter().any(|e| e.event_type == "tool.started"));
    }
}
struct Grant {
    calls: AtomicUsize,
}
#[async_trait]
impl ApprovalHandler for Grant {
    async fn approve(&self, request: &ApprovalRequest) -> bool {
        assert_eq!(request.tool, "workspace.patch");
        self.calls.fetch_add(1, Ordering::SeqCst);
        true
    }
}
#[tokio::test]
async fn child_inherits_parent_approval_even_when_host_and_child_allow() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "old").unwrap();
    let mut parent = resource("parent", Some("child"), &["agent.invoke"], 5);
    parent.definition.agent.permissions.require_approval = vec![Capability::WorkspaceWrite];
    let mut child = resource("child", None, &["workspace.patch"], 4);
    child.definition.agent.permissions.allow = Some(vec![Capability::WorkspaceWrite]);
    let handler = Arc::new(Grant {
        calls: AtomicUsize::new(0),
    });
    let runtime = runtime(
        &tmp,
        vec![child],
        vec![
            (
                "parent",
                fake(vec![invoke("child"), ModelResponse::text("done")]),
            ),
            (
                "child",
                fake(vec![
                    call(
                        "workspace.patch",
                        json!({"path":"a.txt","old_text":"old","new_text":"new"}),
                    ),
                    ModelResponse::text("changed"),
                ]),
            ),
        ],
    )
    .await
    .with_policy(
        RuntimePolicy {
            allow: vec![
                Capability::ReadOnly,
                Capability::AgentDelegate,
                Capability::WorkspaceWrite,
            ],
            require_approval: vec![],
        },
        handler.clone(),
    );
    let (_tx, rx) = watch::channel(false);
    let run = runtime.run(&parent, "Change the file", rx).await.unwrap();
    assert_eq!(run.status, "completed");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "new"
    );
    let child = runtime.store().children(&run.id).await.unwrap().remove(0);
    let events = runtime.store().events(&child.run_id, 0, 100).await.unwrap();
    assert!(events.iter().any(|e| e.event_type == "approval.granted"));
}

#[tokio::test]
async fn parent_and_child_share_model_turn_budget() {
    let tmp = TempDir::new().unwrap();
    let parent = resource("parent", Some("child"), &["agent.invoke"], 2);
    let child = resource("child", None, &[], 12);
    let runtime = runtime(
        &tmp,
        vec![child],
        vec![
            (
                "parent",
                fake(vec![invoke("child"), ModelResponse::text("must not run")]),
            ),
            ("child", fake(vec![ModelResponse::text("done")])),
        ],
    )
    .await;
    let (_tx, rx) = watch::channel(false);
    let run = runtime.run(&parent, "task", rx).await.unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(run.outcome, Some(RunOutcomeKind::Failed));
    assert_eq!(run.reason_code.as_deref(), Some("tool_error"));
    let child = runtime.store().children(&run.id).await.unwrap().remove(0);
    assert_eq!(
        runtime
            .store()
            .get_run(&child.run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "completed"
    );
}

#[tokio::test]
async fn cycles_and_depth_limit_stop_delegation() {
    for cycle in [true, false] {
        let tmp = TempDir::new().unwrap();
        let names = if cycle {
            vec!["root", "child"]
        } else {
            vec!["root", "a", "b", "c", "d", "e"]
        };
        let mut resources = Vec::new();
        let mut providers = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let target = if cycle {
                Some(names[(i + 1) % names.len()])
            } else {
                names.get(i + 1).copied()
            };
            resources.push(resource(
                name,
                target,
                if target.is_some() {
                    &["agent.invoke"]
                } else {
                    &[]
                },
                20,
            ));
            providers.push((
                *name,
                fake(vec![target
                    .map(invoke)
                    .unwrap_or_else(|| ModelResponse::text("unreachable"))]),
            ));
        }
        let parent = resources[0].clone();
        let runtime = runtime(&tmp, resources, providers).await;
        let (_tx, rx) = watch::channel(false);
        let run = runtime.run(&parent, "task", rx).await.unwrap();
        assert_eq!(run.status, "failed");
        let history = runtime.store().history(100).await.unwrap();
        assert_eq!(history.len(), if cycle { 2 } else { 5 });
        assert!(history.iter().all(|r| r.status == "failed"));
    }
}
struct Block;
#[async_trait]
impl ModelProvider for Block {
    async fn generate(&self, _: &ModelRequest) -> ModelResult<ModelResponse> {
        std::future::pending().await
    }
}
#[tokio::test]
async fn cancelling_parent_finalizes_blocked_child_and_invocation() {
    let tmp = TempDir::new().unwrap();
    let parent = resource("parent", Some("child"), &["agent.invoke"], 5);
    let runtime = runtime(
        &tmp,
        vec![resource("child", None, &[], 4)],
        vec![
            ("parent", fake(vec![invoke("child")])),
            ("child", Arc::new(Block)),
        ],
    )
    .await;
    let (tx, rx) = watch::channel(false);
    let cancel = async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let history = runtime.store().history(100).await.unwrap();
                if history.iter().any(|r| r.agent_name == "child") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tx.send(true).unwrap();
    };
    let (run, ()) = tokio::join!(runtime.run(&parent, "task", rx), cancel);
    let run = run.unwrap();
    assert_eq!(run.status, "cancelled");
    let children = runtime.store().children(&run.id).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_ne!(
        runtime
            .store()
            .get_run(&children[0].run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    assert!(runtime
        .store()
        .tool_invocations(&run.id)
        .await
        .unwrap()
        .iter()
        .all(|c| c.status == "failed"));
}

#[tokio::test]
async fn child_cannot_extend_root_deadline() {
    let tmp = TempDir::new().unwrap();
    let mut parent = resource("parent", Some("child"), &["agent.invoke"], 5);
    parent.definition.agent.execution.timeout_seconds = 1;
    parent.version = parent.definition.version().unwrap();
    let runtime = runtime(
        &tmp,
        vec![resource("child", None, &[], 4)],
        vec![
            ("parent", fake(vec![invoke("child")])),
            ("child", Arc::new(Block)),
        ],
    )
    .await;
    let (_tx, rx) = watch::channel(false);
    let run = tokio::time::timeout(Duration::from_secs(3), runtime.run(&parent, "task", rx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, "failed");
    let children = runtime.store().children(&run.id).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(
        runtime
            .store()
            .get_run(&children[0].run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
}

#[tokio::test]
async fn exhausted_tree_budget_prevents_later_tools_in_same_turn() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "old").unwrap();
    let mut parent = resource(
        "parent",
        Some("child"),
        &["agent.invoke", "workspace.patch"],
        2,
    );
    parent
        .definition
        .agent
        .permissions
        .allow
        .as_mut()
        .unwrap()
        .push(Capability::WorkspaceWrite);
    parent.version = parent.definition.version().unwrap();
    let mut response = invoke("child");
    response.tool_calls.push(ToolCall {
        id: "patch".into(),
        name: "workspace.patch".into(),
        arguments: json!({"path":"a.txt","old_text":"old","new_text":"new"}),
    });
    let runtime = runtime(
        &tmp,
        vec![resource("child", None, &[], 2)],
        vec![
            ("parent", fake(vec![response])),
            ("child", fake(vec![ModelResponse::text("done")])),
        ],
    )
    .await
    .with_policy(
        RuntimePolicy {
            allow: vec![
                Capability::ReadOnly,
                Capability::AgentDelegate,
                Capability::WorkspaceWrite,
            ],
            require_approval: vec![],
        },
        Arc::new(DenyApprovals),
    );
    let (_tx, rx) = watch::channel(false);
    let run = runtime.run(&parent, "task", rx).await.unwrap();
    assert_eq!(run.status, "failed");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "old"
    );
    assert_eq!(
        runtime
            .store()
            .tool_invocations(&run.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// DESIGN-0010 first-target flow with deterministic models and real local tools.
#[tokio::test]
async fn implementer_edits_tests_and_delegates_review() {
    struct DemoApproval;
    #[async_trait]
    impl ApprovalHandler for DemoApproval {
        async fn approve(&self, request: &ApprovalRequest) -> bool {
            matches!(request.tool.as_str(), "workspace.patch" | "process.exec")
        }
    }
    let tmp = TempDir::new().unwrap();
    let old = "def valid(limit):\n    return True\n";
    let new = "def valid(limit):\n    return limit > 0\n";
    std::fs::write(tmp.path().join("config.py"), old).unwrap();
    let mut parent = resource(
        "implementer",
        Some("reviewer"),
        &[
            "workspace.read",
            "workspace.patch",
            "process.exec",
            "agent.invoke",
        ],
        8,
    );
    parent.definition.agent.permissions.require_approval =
        vec![Capability::WorkspaceWrite, Capability::ProcessExecute];
    parent.version = parent.definition.version().unwrap();
    let child = resource("reviewer", None, &["workspace.read"], 2);
    let mut responses = vec![
        call("workspace.read", json!({"path":"config.py"})),
        call(
            "workspace.patch",
            json!({"path":"config.py","old_text":old,"new_text":new}),
        ),
        call(
            "process.exec",
            json!({"argv":["python3","-B","-c","from config import valid; assert valid(1); assert not valid(0); assert not valid(-1)"]}),
        ),
        invoke("reviewer"),
        ModelResponse::text("Added positive-limit validation; tests and review passed."),
    ];
    for (i, response) in responses.iter_mut().enumerate() {
        for tool in &mut response.tool_calls {
            tool.id = format!("step-{i}");
        }
    }
    let runtime = runtime(
        &tmp,
        vec![child],
        vec![
            ("implementer", fake(responses)),
            (
                "reviewer",
                fake(vec![
                    call("workspace.read", json!({"path":"config.py"})),
                    ModelResponse::text("Positive limits accepted; nonpositive limits rejected."),
                ]),
            ),
        ],
    )
    .await
    .with_policy(RuntimePolicy::default(), Arc::new(DemoApproval));
    let (_tx, rx) = watch::channel(false);
    let run = runtime
        .run(&parent, "Add validation to the configuration loader", rx)
        .await
        .unwrap();
    assert_eq!(run.status, "completed", "{:?}", run.error);
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("config.py")).unwrap(),
        new
    );
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|call| call.status == "completed"));
    assert_eq!(calls[2].result.as_ref().unwrap()["exit_code"], 0);
    let child_id = &runtime.store().children(&run.id).await.unwrap()[0].run_id;
    let review_calls = runtime.store().tool_invocations(child_id).await.unwrap();
    assert_eq!(review_calls[0].result.as_ref().unwrap()["text"], new);
    let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "approval.granted")
            .count(),
        2
    );
}
