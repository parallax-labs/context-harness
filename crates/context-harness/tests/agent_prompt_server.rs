use serde_json::{json, Value};
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::Command;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

async fn isolated_prompt_server(use_env_config: bool) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("workspace");
    let global = tmp.path().join("global");
    let selected = tmp.path().join("selected");
    // Neither the config nor resource discovery may touch these unrelated layers.
    for path in [
        root.join(".ctx/config.toml"),
        global.join("config.toml"),
        root.join(".ctx/agents/broken.toml"),
        global.join("agents/broken.toml"),
    ] {
        write(&path, "not valid TOML [");
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let db = root.join("uncreated/database.sqlite");
    let lua = selected.join("legacy.lua");
    write(&lua, "agent = {name = 'legacy_lua', description = 'Legacy Lua', tools = {'search'}}\nfunction agent.resolve(args, config, context) return {system = 'Lua topic: ' .. args.topic, tools = {'search'}} end");
    let config = selected.join("config.toml");
    write(
        &config,
        &format!(
            r#"
[db]
path = {db:?}
[chunking]
max_tokens = 700
[retrieval]
final_limit = 12
[server]
bind = "127.0.0.1:{port}"
[models.reasoning]
provider = "openai"
model = "unused-test-model"
api_key_env = "CTX_PROMPT_SERVER_MISSING_KEY"
[mcp_servers.unstarted]
command = "ctx-prompt-test-command-that-does-not-exist"
[agents.inline.legacy_inline]
description = "Legacy inline"
tools = ["get"]
system_prompt = "Inline prompt"
[agents.script.legacy_lua]
path = {lua:?}
"#,
            db = db.to_str().unwrap(),
            lua = lua.to_str().unwrap(),
        ),
    );
    write(
        &selected.join("agents/researcher.toml"),
        r#"
[agent]
name = "researcher"
description = "Standalone prompt"
model = "reasoning"
tools = ["mcp.unstarted.lookup", "workspace.patch"]
[prompt]
system = "Use project context before answering."
"#,
    );
    let stderr_path = tmp.path().join("server.stderr");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ctx"));
    command
        .current_dir(&root)
        .env("CTX_CONFIG_DIR", &global)
        .env_remove("CTX_CONFIG")
        .env_remove("CTX_PROMPT_SERVER_MISSING_KEY")
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(&stderr_path).unwrap()))
        .kill_on_drop(true);
    if use_env_config {
        command.env("CTX_CONFIG", &config);
    } else {
        command.arg("--config").arg(&config);
    }
    let mut server = command.args(["serve", "mcp"]).spawn().unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let ready = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = server.try_wait().unwrap() {
                panic!(
                    "server exited {status}: {}",
                    fs::read_to_string(&stderr_path).unwrap()
                );
            }
            if let Ok(response) = client.get(format!("{base}/agents/list")).send().await {
                if response.status().is_success() {
                    break response.json::<Value>().await.unwrap();
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("prompt server did not become ready");
    let agents = ready["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 3, "unexpected agents: {ready}");
    for name in ["researcher", "legacy_inline", "legacy_lua"] {
        assert!(agents.iter().any(|entry| entry["name"] == name));
    }
    for (name, args, expected) in [
        (
            "researcher",
            json!({}),
            "Use project context before answering.",
        ),
        ("legacy_inline", json!({}), "Inline prompt"),
        (
            "legacy_lua",
            json!({"topic": "compatibility"}),
            "Lua topic: compatibility",
        ),
    ] {
        let response = client
            .post(format!("{base}/agents/{name}/prompt"))
            .json(&args)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{name}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["system"], expected);
    }
    let tools: Value = client
        .get(format!("{base}/tools/list"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let serialized = serde_json::to_string(&tools).unwrap();
    assert!(!serialized.contains("workspace.patch"));
    assert!(!serialized.contains("mcp.unstarted.lookup"));
    assert!(!db.exists());
    assert!(!db.parent().unwrap().exists());
    assert!(!root.join(".ctx/runs").exists());
    assert!(!selected.join(".ctx/runs").exists());
    server.kill().await.unwrap();
    server.wait().await.unwrap();
}

#[tokio::test]
async fn explicit_config_serves_sibling_resources_without_runtime_side_effects() {
    isolated_prompt_server(false).await;
}

#[tokio::test]
async fn environment_config_serves_sibling_resources_without_runtime_side_effects() {
    isolated_prompt_server(true).await;
}
