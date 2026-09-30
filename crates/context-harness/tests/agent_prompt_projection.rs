use context_harness::{
    agent_resource::{ResourceDirectory, ResourceScope},
    agents::{AgentRegistry, TomlAgent},
    config::Config,
    mcp::McpBridge,
    server::register_resource_prompts,
    traits::ToolRegistry,
    workspace::{ServerMode, WorkspaceRouter},
};
use rmcp::{model::GetPromptRequestParams, ServiceExt};
use std::sync::Arc;
use tempfile::TempDir;

fn fixture() -> (TempDir, Config, Vec<ResourceDirectory>) {
    let root = TempDir::new().unwrap();
    let mut config = Config::minimal();
    config.db.path = root.path().join("must-not-be-created.sqlite");
    config.models = toml::from_str::<Config>(
        r#"
[db]
path='unused'
[chunking]
max_tokens=700
[retrieval]
final_limit=12
[server]
bind='127.0.0.1:0'
[models.reasoning]
provider='openai'
model='unused-model'
api_key_env='CTX_PROMPT_TEST_NO_CREDENTIAL'
"#,
    )
    .unwrap()
    .models;
    std::fs::write(
        root.path().join("researcher.toml"),
        r#"
[agent]
name='researcher'
description='Project context'
model='reasoning'
tools=['workspace.patch', 'mcp.offline.echo']
[agent.permissions]
allow=['workspace_write']
[prompt]
system='Use project context before answering.'
"#,
    )
    .unwrap();
    let dirs = vec![ResourceDirectory {
        path: root.path().into(),
        scope: ResourceScope::Explicit,
    }];
    (root, config, dirs)
}
fn legacy(name: &str) -> Box<TomlAgent> {
    Box::new(TomlAgent::new(
        name.into(),
        "Legacy prompt".into(),
        vec![],
        "Legacy system".into(),
    ))
}

#[tokio::test]
async fn mcp_projects_resources_without_execution_and_preserves_legacy_prompts() {
    let (_root, config, dirs) = fixture();
    let mut agents = AgentRegistry::new();
    agents.register(legacy("legacy"));
    let mut extra = AgentRegistry::new();
    extra.register(legacy("extension"));
    register_resource_prompts(&config, &dirs, &mut agents, &extra).unwrap();
    let bridge = McpBridge::new(
        Arc::new(WorkspaceRouter::single(Arc::new(config.clone()))),
        ServerMode::Compat,
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(ToolRegistry::new()),
        Arc::new(agents),
        Arc::new(extra),
    );
    let (server_io, client_io) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { bridge.serve(server_io).await.unwrap() });
    let client = ().serve(client_io).await.unwrap();
    let server = server.await.unwrap();
    let prompts = client.list_prompts(None).await.unwrap();
    assert_eq!(prompts.prompts.len(), 3);
    let resource = prompts
        .prompts
        .iter()
        .find(|p| p.name == "researcher")
        .unwrap();
    assert_eq!(resource.description.as_deref(), Some("Project context"));
    assert!(resource.arguments.is_none());
    for (name, text) in [
        ("researcher", "Use project context before answering."),
        ("legacy", "Legacy system"),
        ("extension", "Legacy system"),
    ] {
        let result = client
            .get_prompt(GetPromptRequestParams {
                name: name.into(),
                arguments: None,
                meta: None,
            })
            .await
            .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"]["text"], text);
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
    }
    // Runtime-only declarations never register tools, start a provider/server,
    // or create run history as a side effect of resolving an MCP prompt.
    let tools = client.list_tools(None).await.unwrap();
    assert!(!tools
        .tools
        .iter()
        .any(|t| t.name == "workspace.patch" || t.name.starts_with("mcp.")));
    assert!(!config.db.path.exists());
    client.cancel().await.unwrap();
    server.cancel().await.unwrap();
}

#[test]
fn resource_collisions_fail_before_partial_registration() {
    let (root, config, dirs) = fixture();
    let source = std::fs::read_to_string(root.path().join("researcher.toml")).unwrap();
    std::fs::write(
        root.path().join("first.toml"),
        source.replace("researcher", "aaa_first"),
    )
    .unwrap();
    for extra_collision in [false, true] {
        let mut agents = AgentRegistry::new();
        let mut extra = AgentRegistry::new();
        if extra_collision {
            extra.register(legacy("researcher"));
        } else {
            agents.register(legacy("researcher"));
        }
        let before = agents.len();
        let error = register_resource_prompts(&config, &dirs, &mut agents, &extra).unwrap_err();
        assert!(error
            .to_string()
            .contains("conflicts with registered agent 'researcher'"));
        assert_eq!(agents.len(), before);
    }
}
