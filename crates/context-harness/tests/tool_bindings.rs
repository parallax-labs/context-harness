use context_harness::{
    agent_model::{fake::FakeModel, FinishReason, ModelResponse, ToolCall},
    agent_resource::{AgentResource, LoadedAgentResource},
    agent_resource::{ResourceDirectory, ResourceScope},
    agent_runtime::AgentRuntime,
    config::{Config, ResolvedConfig},
    ctx_dirs::ConfigSourceKind,
    tool_binding::{
        core_metadata_catalog, load_named_resource, load_resources, resolve_resources,
        resource_directories,
    },
};
use serde_json::Value;
use std::{fs, path::Path, process::Command, sync::Arc};
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

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn config() -> Config {
    toml::from_str(CONFIG).unwrap()
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
