use async_trait::async_trait;
use context_harness::{
    agent_model::{fake::FakeModel, FinishReason, ModelResponse, ToolCall},
    agent_resource::{AgentResource, Capability, LoadedAgentResource},
    agent_resource::{ResourceDirectory, ResourceScope},
    agent_runtime::AgentRuntime,
    app_store::SqliteAppStore,
    chunk::chunk_text,
    config::{Config, ResolvedConfig},
    ctx_dirs::ConfigSourceKind,
    models::Document,
    tool_binding::{
        bind_resources, core_catalog, core_metadata_catalog, load_named_resource, load_resources,
        resolve_resources, resource_directories, HostToolAuthority,
    },
    traits::ToolContext,
};
use context_harness_core::store::Store;
use serde_json::Value;
use std::{fs, path::Path, process::Command, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::sync::watch;

const CONFIG: &str = r#"
[db]
path = ".ctx/data/ctx.sqlite"
[chunking]
max_tokens = 700
[retrieval]
final_limit = 12
[server]
bind = "127.0.0.1:0"
[connectors.filesystem.decisions]
root = "."
[connectors.filesystem.private]
root = "."
"#;

const RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "release.read"
implementation = "builtin.scoped_file_read"
description = "Read release fixture files"
[config]
root = "release"
[fixed]
max_bytes = 65536
[restrictions]
paths = ["release"]
max_output_bytes = 65536
"#;

const SEARCH_RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "decisions.search"
implementation = "builtin.retrieval.search"
description = "Search release decisions"
[config]
source = "filesystem:decisions"
[fixed]
limit = 5
[restrictions]
sources = ["filesystem:decisions"]
max_output_bytes = 65536
"#;

const GET_RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "decisions.get"
implementation = "builtin.retrieval.get"
description = "Get a release decision"
[config]
source = "filesystem:decisions"
[restrictions]
sources = ["filesystem:decisions"]
max_output_bytes = 65536
"#;

const MCP_RESOURCE: &str = r#"
schema_version = 1
[tool]
name = "fixture.echo_alias"
implementation = "mcp.fixture.echo"
description = "Echo a bound fixture value"
[fixed]
text = "bound"
[restrictions]
max_output_bytes = 65536
"#;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn config() -> Config {
    toml::from_str(CONFIG).unwrap()
}

async fn seed_document(config: &Config, id: &str, source: &str, body: &str) -> String {
    SqliteAppStore::initialize_config(config).await.unwrap();
    let store = SqliteAppStore::connect(config).await.unwrap();
    let document = Document {
        id: id.into(),
        source: source.into(),
        source_id: format!("{id}.md"),
        source_url: None,
        title: Some(id.into()),
        author: None,
        created_at: 1,
        updated_at: 2,
        content_type: "text/plain".into(),
        body: body.into(),
        metadata_json: "{}".into(),
        raw_json: None,
        dedup_hash: format!("hash-{id}"),
    };
    let stored_id = store.upsert_document(&document).await.unwrap();
    store
        .replace_chunks(&stored_id, &chunk_text(&stored_id, body, 700), None)
        .await
        .unwrap();
    store.close().await;
    stored_id
}

struct BlockingModel;

#[async_trait]
impl context_harness::agent_model::ModelProvider for BlockingModel {
    async fn generate(
        &self,
        _request: &context_harness::agent_model::ModelRequest,
    ) -> context_harness::agent_model::ModelResult<ModelResponse> {
        std::future::pending().await
    }
}

#[test]
fn layered_discovery_requires_explicit_whole_resource_override() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let workspace = temp.path().join("workspace");
    write(&global.join("tools/read.toml"), RESOURCE);
    write(&workspace.join(".ctx/tools/read.toml"), RESOURCE);
    let resolved = ResolvedConfig {
        config: config(),
        path: Some(workspace.join(".ctx/config.toml")),
        source: ConfigSourceKind::Workspace,
    };
    let directories = resource_directories(&resolved, &workspace, &global);
    assert!(load_resources(&directories, &resolved.config)
        .unwrap_err()
        .to_string()
        .contains("override"));

    write(
        &workspace.join(".ctx/tools/read.toml"),
        &RESOURCE.replace("name =", "override = true\nname ="),
    );
    let loaded = load_resources(&directories, &resolved.config).unwrap();
    assert_eq!(loaded["release.read"].scope, ResourceScope::Workspace);
}

#[test]
fn explicit_config_isolates_resources_and_resolution_fixes_public_arguments() {
    let temp = TempDir::new().unwrap();
    let explicit = temp.path().join("selected/config.toml");
    let ambient = temp.path().join("workspace");
    write(&explicit, CONFIG);
    write(
        &explicit.parent().unwrap().join("tools/read.toml"),
        RESOURCE,
    );
    write(
        &ambient.join(".ctx/tools/broken.toml"),
        "this is not valid TOML",
    );
    let resolved = ResolvedConfig {
        config: config(),
        path: Some(explicit),
        source: ConfigSourceKind::Explicit,
    };
    let directories = resource_directories(&resolved, &ambient, &temp.path().join("global"));
    let loaded = load_resources(&directories, &resolved.config).unwrap();
    let bindings = resolve_resources(loaded, &core_metadata_catalog().unwrap()).unwrap();
    let binding = &bindings["release.read"].binding;
    assert!(binding.public_schema["properties"]
        .get("max_bytes")
        .is_none());
    assert_eq!(binding.fixed["max_bytes"], 65536);
    assert_eq!(binding.capabilities.len(), 1);
}

#[test]
fn cli_inspection_is_static_and_machine_readable() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("fixture/config.toml");
    write(&config_path, CONFIG);
    write(&temp.path().join("fixture/tools/read.toml"), RESOURCE);

    let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "tool",
            "bindings",
            "show",
            "release.read",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["scope"], "explicit");
    assert_eq!(
        value["binding"]["implementation_id"],
        "builtin.scoped_file_read"
    );
    assert!(value["binding"]["public_schema"]["properties"]
        .get("max_bytes")
        .is_none());
    assert!(!temp.path().join(".ctx/data/ctx.sqlite").exists());

    let validation = Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "tool",
            "bindings",
            "validate",
            "release.read",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(validation.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&validation.stdout).unwrap()["valid"],
        true
    );
}

#[test]
fn mcp_alias_inspection_is_static_and_marks_remote_metadata_unresolved() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("fixture/config.toml");
    write(&config_path, CONFIG);
    write(
        &temp.path().join("fixture/tools/echo-alias.toml"),
        MCP_RESOURCE,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "tool",
            "bindings",
            "show",
            "fixture.echo_alias",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["binding"]["implementation_id"], "mcp.fixture.echo");
    assert_eq!(value["binding"]["remote_metadata"], "unresolved");
    assert_eq!(value["binding"]["fixed"]["text"], "bound");
    assert!(!temp.path().join("mcp-pid").exists());
    assert!(!temp.path().join(".ctx/data/ctx.sqlite").exists());
}

#[test]
fn resolution_rejects_unknown_implementations_and_unenforceable_scopes() {
    let temp = TempDir::new().unwrap();
    let directory = ResourceDirectory {
        path: temp.path().to_path_buf(),
        scope: ResourceScope::Workspace,
    };
    write(
        &temp.path().join("unknown.toml"),
        &RESOURCE.replace("builtin.scoped_file_read", "rust.missing"),
    );
    let loaded = load_resources(std::slice::from_ref(&directory), &config()).unwrap();
    let error = resolve_resources(loaded, &core_metadata_catalog().unwrap()).unwrap_err();
    assert!(format!("{error:#}").contains("unknown tool implementation"));

    write(
        &temp.path().join("unknown.toml"),
        &RESOURCE.replace(
            "paths = [\"release\"]",
            "sources = [\"filesystem:release\"]",
        ),
    );
    let loaded = load_resources(&[directory], &config()).unwrap();
    let error = resolve_resources(loaded, &core_metadata_catalog().unwrap()).unwrap_err();
    assert!(format!("{error:#}").contains("cannot enforce"));
}

#[test]
fn targeted_loading_ignores_unrelated_broken_resources() {
    let temp = TempDir::new().unwrap();
    write(&temp.path().join("selected.toml"), RESOURCE);
    write(&temp.path().join("unrelated.toml"), "not = [valid");
    let directory = ResourceDirectory {
        path: temp.path().to_path_buf(),
        scope: ResourceScope::Workspace,
    };
    assert!(load_resources(std::slice::from_ref(&directory), &config()).is_err());
    let selected = load_named_resource(&[directory], &config(), "release.read").unwrap();
    assert_eq!(selected.len(), 1);
    assert!(selected.contains_key("release.read"));
}

#[tokio::test]
async fn scoped_reader_alias_executes_through_the_common_runtime_lifecycle() {
    let temp = TempDir::new().unwrap();
    write(&temp.path().join("release/summary.md"), "release evidence");
    let tools = temp.path().join("tool-resources");
    write(&tools.join("read.toml"), RESOURCE);
    let tool_directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];

    let agent_source = r#"
[agent]
name = "reader"
model = "test"
tools = ["release.read"]
[agent.execution]
max_turns = 3
timeout_seconds = 10
[prompt]
system = "Read the release evidence."
"#;
    let definition = AgentResource::parse(agent_source).unwrap();
    let agent = LoadedAgentResource {
        path: "agent.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    };
    let mut call = ModelResponse::text("");
    call.finish_reason = FinishReason::ToolCalls;
    call.tool_calls = vec![ToolCall {
        name: "release.read".into(),
        id: "read-1".into(),
        arguments: serde_json::json!({"path":"summary.md"}),
    }];
    let provider = Arc::new(FakeModel::new([
        Ok(call),
        Ok(ModelResponse::text("reviewed release evidence")),
    ]));
    let mut models = context_harness::agent_model::ModelRegistry::default();
    models
        .register("test", "fake", "scripted", provider)
        .unwrap();
    let mut cfg = config();
    cfg.db.path = temp.path().join("ctx.sqlite");
    let runtime = AgentRuntime::new(cfg, temp.path(), models)
        .await
        .unwrap()
        .with_tool_bindings(&tool_directories)
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime.run(&agent, "Review", cancel).await.unwrap();
    assert_eq!(run.status, "completed");
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool_name, "release.read");
    assert!(calls[0]
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("release evidence"));
}

#[tokio::test]
async fn retrieval_aliases_enforce_the_bound_source_without_name_dispatch() {
    let temp = TempDir::new().unwrap();
    let tools = temp.path().join("tool-resources");
    write(&tools.join("search.toml"), SEARCH_RESOURCE);
    write(&tools.join("get.toml"), GET_RESOURCE);
    let tool_directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let mut cfg = config();
    cfg.db.path = temp.path().join("ctx.sqlite");
    let decision_id = seed_document(
        &cfg,
        "decision-a",
        "filesystem:decisions",
        "Approved deployment target",
    )
    .await;
    let private_id = seed_document(
        &cfg,
        "private-a",
        "filesystem:private",
        "Private deployment target",
    )
    .await;

    let loaded = load_resources(&tool_directories, &cfg).unwrap();
    let mut authority = HostToolAuthority::new(temp.path(), vec![Capability::ReadOnly]).unwrap();
    authority.enroll_path(temp.path()).unwrap();
    let ungranted = bind_resources(
        &loaded,
        &core_catalog().unwrap(),
        Arc::new(authority.clone()),
    )
    .await
    .err()
    .unwrap();
    assert!(format!("{ungranted:#}").contains("retrieval source is not granted by the host"));
    authority
        .enroll_source("filesystem:decisions".into())
        .unwrap();
    let bindings = bind_resources(&loaded, &core_catalog().unwrap(), Arc::new(authority))
        .await
        .unwrap();
    let context = ToolContext::new(Arc::new(cfg.clone()));
    let boundary_error = bindings
        .find("decisions.get")
        .unwrap()
        .execute(serde_json::json!({"id": private_id}), &context)
        .await
        .unwrap_err();
    assert!(boundary_error
        .to_string()
        .contains("outside the bound source"));
    let agent_source = r#"
[agent]
name = "decision-reviewer"
model = "test"
tools = ["decisions.search", "decisions.get"]
[agent.execution]
max_turns = 4
timeout_seconds = 10
[prompt]
system = "Review release decisions."
"#;
    let definition = AgentResource::parse(agent_source).unwrap();
    let agent = LoadedAgentResource {
        path: "agent.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    };

    let mut search = ModelResponse::text("");
    search.finish_reason = FinishReason::ToolCalls;
    search.tool_calls = vec![ToolCall {
        name: "decisions.search".into(),
        id: "search-1".into(),
        arguments: serde_json::json!({"query":"deployment"}),
    }];
    let mut get = ModelResponse::text("");
    get.finish_reason = FinishReason::ToolCalls;
    get.tool_calls = vec![ToolCall {
        name: "decisions.get".into(),
        id: "get-1".into(),
        arguments: serde_json::json!({"id": decision_id}),
    }];
    let provider = Arc::new(FakeModel::new([
        Ok(search),
        Ok(get),
        Ok(ModelResponse::text("reviewed decision")),
    ]));
    let mut models = context_harness::agent_model::ModelRegistry::default();
    models
        .register("test", "fake", "scripted", provider)
        .unwrap();
    let runtime = AgentRuntime::new(cfg.clone(), temp.path(), models)
        .await
        .unwrap()
        .with_tool_bindings(&tool_directories)
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let run = runtime.run(&agent, "Review", cancel).await.unwrap();
    assert_eq!(run.status, "completed");
    let calls = runtime.store().tool_invocations(&run.id).await.unwrap();
    assert_eq!(calls.len(), 2);
    let search_result = calls[0].result.as_ref().unwrap().to_string();
    assert!(search_result.contains("decision-a"));
    assert!(!search_result.contains("private-a"));
    assert!(calls[1]
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("Approved deployment target"));

    let mut cross_source = ModelResponse::text("");
    cross_source.finish_reason = FinishReason::ToolCalls;
    cross_source.tool_calls = vec![ToolCall {
        name: "decisions.get".into(),
        id: "get-private".into(),
        arguments: serde_json::json!({"id": private_id}),
    }];
    let provider = Arc::new(FakeModel::new([Ok(cross_source)]));
    let mut models = context_harness::agent_model::ModelRegistry::default();
    models
        .register("test", "fake", "scripted", provider)
        .unwrap();
    let denied_runtime = AgentRuntime::new(cfg, temp.path(), models)
        .await
        .unwrap()
        .with_tool_bindings(&tool_directories)
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let denied = denied_runtime
        .run(&agent, "Read private", cancel)
        .await
        .unwrap();
    assert_eq!(denied.status, "failed");
    let denied_calls = denied_runtime
        .store()
        .tool_invocations(&denied.id)
        .await
        .unwrap();
    assert_eq!(denied_calls.len(), 1);
    assert_eq!(
        denied_calls[0].error.as_deref(),
        Some("context tool execution failed")
    );
}

#[tokio::test]
async fn changed_binding_identity_rejects_resume_and_history_keeps_snapshot() {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("release")).unwrap();
    fs::create_dir(temp.path().join("release-next")).unwrap();
    let tools = temp.path().join("tool-resources");
    let resource_path = tools.join("read.toml");
    write(&resource_path, RESOURCE);
    let tool_directories = [ResourceDirectory {
        path: tools,
        scope: ResourceScope::Workspace,
    }];
    let definition = AgentResource::parse(
        r#"
[agent]
name = "reader"
model = "test"
tools = ["release.read"]
[agent.execution]
max_turns = 3
timeout_seconds = 10
[prompt]
system = "Read release evidence."
"#,
    )
    .unwrap();
    let agent = LoadedAgentResource {
        path: "agent.toml".into(),
        scope: ResourceScope::Workspace,
        version: definition.version().unwrap(),
        definition,
    };
    let mut cfg = config();
    cfg.db.path = temp.path().join("ctx.sqlite");
    let models = || {
        let mut models = context_harness::agent_model::ModelRegistry::default();
        models
            .register("test", "fake", "blocking", Arc::new(BlockingModel))
            .unwrap();
        models
    };
    let runtime = Arc::new(
        AgentRuntime::new(cfg.clone(), temp.path(), models())
            .await
            .unwrap()
            .with_tool_bindings(&tool_directories)
            .await
            .unwrap(),
    );
    let copy = agent.clone();
    let running = runtime.clone();
    let (_sender, cancel) = watch::channel(false);
    let task = tokio::spawn(async move { running.run(&copy, "Review", cancel).await });
    let run_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(run) = runtime.store().history(1).await.unwrap().first() {
                let events = runtime.store().events(&run.id, 0, 100).await.unwrap();
                if events
                    .iter()
                    .any(|event| event.event_type == "model.requested")
                {
                    let resolved = events
                        .iter()
                        .find(|event| event.event_type == "context.resolved")
                        .unwrap();
                    assert_eq!(
                        resolved.payload["tool_bindings"]["release.read"]["implementation_id"],
                        "builtin.scoped_file_read"
                    );
                    break run.id.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    let changed = RESOURCE
        .replace("root = \"release\"", "root = \"release-next\"")
        .replace("paths = [\"release\"]", "paths = [\"release-next\"]");
    write(&resource_path, &changed);
    let changed_runtime = AgentRuntime::new(cfg, temp.path(), models())
        .await
        .unwrap()
        .with_tool_bindings(&tool_directories)
        .await
        .unwrap();
    let (_sender, cancel) = watch::channel(false);
    let error = changed_runtime
        .resume(&run_id, &agent, cancel)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("checkpoint binding changed or is invalid"));
}
