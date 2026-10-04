use context_harness::agent_resource::{
    catalog, load_resources, resource_directories, AgentResource, Capability, ModelDefinition,
    ResourceDirectory, ResourceScope,
};
use context_harness::config::{Config, ResolvedConfig};
use context_harness::ctx_dirs::ConfigSourceKind;
use context_harness::profiles::ProfileRegistry;
use context_harness::traits::ToolContext;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;
use tempfile::TempDir;

const RESOURCE: &str = r#"
[agent]
name = "researcher"
description = "Search project context"
model = "reasoning"
tools = ["search", "get"]
[prompt]
system = "Use project context before answering."
"#;
const CONFIG: &str = r#"
[db]
path = ".ctx/data/ctx.sqlite"
[chunking]
max_tokens = 700
[retrieval]
final_limit = 12
[server]
bind = "127.0.0.1:0"
[models.reasoning]
provider = "fake"
model = "test-model"
api_key_env = "CTX_TEST_UNUSED_KEY"
"#;

fn config() -> Config {
    toml::from_str(CONFIG).unwrap()
}
fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}
fn dir(root: &Path, scope: ResourceScope) -> ResourceDirectory {
    ResourceDirectory {
        path: root.to_path_buf(),
        scope,
    }
}
fn run(root: &Path, global: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(root)
        .env("CTX_CONFIG_DIR", global)
        .env_remove("CTX_CONFIG")
        .env_remove("CTX_TEST_UNUSED_KEY")
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn resource_defaults_hashes_and_explicit_permissions() {
    let resource = AgentResource::parse(RESOURCE).unwrap();
    assert_eq!(resource.agent.execution.max_turns, 12);
    assert_eq!(resource.agent.execution.timeout_seconds, 300);
    assert_eq!(
        resource.agent.permissions.allowed(),
        vec![Capability::ReadOnly]
    );
    let same = AgentResource::parse(&format!("# comment\n{RESOURCE}\n")).unwrap();
    assert_eq!(resource.version().unwrap(), same.version().unwrap());
    let changed =
        AgentResource::parse(&RESOURCE.replace("before answering", "before writing")).unwrap();
    assert_ne!(resource.version().unwrap(), changed.version().unwrap());
    let policy = AgentResource::parse(&format!(
        "{RESOURCE}\n[agent.permissions]\nallow = []\nrequire_approval = ['workspace_write']"
    ))
    .unwrap();
    assert!(policy.agent.permissions.allowed().is_empty());
    assert_eq!(
        policy.agent.permissions.require_approval,
        vec![Capability::WorkspaceWrite]
    );
}

#[test]
fn invalid_or_unknown_fields_fail_instead_of_silently_weakening_policy() {
    for input in [
        RESOURCE.replace("tools =", "toools ="),
        format!("{RESOURCE}\n[agent.permissions]\nmode = 'unrestricted'"),
        format!("{RESOURCE}\n[agent.permissions]\nallow = ['typo']"),
        format!("{RESOURCE}\n[agent.permissions]\nmode = 'read-only'\nallow = ['workspace_write']"),
        format!("{RESOURCE}\n[agent.permissions]\nallow = ['workspace_write']\nrequire_approval = ['workspace_write']"),
        format!("{RESOURCE}\n[agent.permissions]\nallow = ['read_only', 'read_only']"),
        format!("{RESOURCE}\n[agent.execution]\nmax_turns = 0"),
        format!("{RESOURCE}\n[agent.execution]\ntimeout_seconds = -1"),
        format!("{RESOURCE}\n[agent.execution]\ntimeout_seconds = 0"),
        format!("{RESOURCE}\n[agent.execution]\ntimeout = 12"),
        RESOURCE.replace("[\"search\", \"get\"]", "[\"search\", \"search\"]"),
        RESOURCE.replace("researcher", "bad/name"),
        RESOURCE.replace("Use project context before answering.", " "),
    ] {
        assert!(AgentResource::parse(&input).is_err(), "accepted {input}");
    }
    assert!(toml::from_str::<ModelDefinition>(
        "provider = 'fake'\nmodel = 'test'\napi_key = 'secret'"
    )
    .is_err());
}

#[test]
fn workspace_overrides_replace_entire_definition_only_when_explicit() {
    let tmp = TempDir::new().unwrap();
    let global = tmp.path().join("global");
    let workspace = tmp.path().join("project");
    write(
        &global.join("review.toml"),
        &format!("{RESOURCE}\n[agent.permissions]\nallow = ['process_execute']"),
    );
    write(&workspace.join("review.toml"), RESOURCE);
    let directories = [
        dir(&global, ResourceScope::Global),
        dir(&workspace, ResourceScope::Workspace),
    ];
    assert!(load_resources(&directories, &config())
        .unwrap_err()
        .to_string()
        .contains("override"));
    write(
        &workspace.join("review.toml"),
        &RESOURCE.replace("name =", "override = true\nname ="),
    );
    let loaded = load_resources(&directories, &config()).unwrap();
    let selected = &loaded["researcher"];
    assert_eq!(selected.scope, ResourceScope::Workspace);
    assert_eq!(
        selected.definition.agent.permissions.allowed(),
        vec![Capability::ReadOnly]
    );
    assert!(selected.path.starts_with(workspace.canonicalize().unwrap()));
}

#[test]
fn duplicate_names_unknown_models_and_legacy_collisions_are_errors() {
    let tmp = TempDir::new().unwrap();
    let directories = [dir(tmp.path(), ResourceScope::Workspace)];
    write(&tmp.path().join("first.toml"), RESOURCE);
    write(
        &tmp.path().join("second.toml"),
        &RESOURCE.replace("name =", "override = true\nname ="),
    );
    assert!(load_resources(&directories, &config())
        .unwrap_err()
        .to_string()
        .contains("duplicate"));
    fs::remove_file(tmp.path().join("second.toml")).unwrap();
    let error = load_resources(&directories, &Config::minimal())
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown model 'reasoning'"));
    assert!(error.contains("first.toml"));
    let legacy: Config = toml::from_str(&format!("{CONFIG}\n[agents.inline.researcher]\ndescription = 'legacy'\ntools = []\nsystem_prompt = 'old'")).unwrap();
    assert!(load_resources(&directories, &legacy)
        .unwrap_err()
        .to_string()
        .contains("conflicts with profile"));
}

#[test]
fn explicit_and_pinned_resource_paths_never_import_ambient_directories() {
    let root = Path::new("/project");
    let global = Path::new("/global");
    for source in [ConfigSourceKind::Explicit, ConfigSourceKind::Env] {
        let resolved = ResolvedConfig {
            config: config(),
            path: Some("custom/config.toml".into()),
            source,
        };
        let dirs = resource_directories(&resolved, root, global);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].path, root.join("custom/agents"));
        assert_eq!(dirs[0].scope, ResourceScope::Explicit);
    }
    let resolved = ResolvedConfig {
        config: config(),
        path: None,
        source: ConfigSourceKind::BuiltIn,
    };
    let dirs = resource_directories(&resolved, root, global);
    assert_eq!(dirs[0].path, global.join("agents"));
    assert_eq!(dirs[1].path, root.join(".ctx/agents"));
}

#[tokio::test]
async fn standalone_prompt_uses_existing_agent_trait_and_legacy_agents_coexist() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("agents/researcher.toml"), RESOURCE);
    write(&tmp.path().join("legacy.lua"), "agent = { name = 'legacy_lua', description = 'Lua', tools = {'search'} }\nfunction agent.resolve(args, config, context) return {system = 'Lua prompt', tools = {'search'}} end");
    let cfg: Config = toml::from_str(&format!("{CONFIG}\n[agents.inline.legacy]\ndescription = 'old'\ntools = ['get']\nsystem_prompt = 'Legacy prompt'\n[agents.script.legacy_lua]\npath = {:?}", tmp.path().join("legacy.lua").to_str().unwrap())).unwrap();
    let entries = catalog(
        &cfg,
        &[dir(&tmp.path().join("agents"), ResourceScope::Workspace)],
    )
    .unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries["legacy_lua"].source, "lua");
    let resource = entries["researcher"].resource.as_ref().unwrap();
    let mut registry = ProfileRegistry::from_config(&cfg).unwrap();
    registry.register(Box::new(resource.definition.prompt_profile()));
    let ctx = ToolContext::new(Arc::new(cfg));
    let prompt = registry
        .find("researcher")
        .unwrap()
        .resolve(serde_json::json!({}), &ctx)
        .await
        .unwrap();
    assert_eq!(prompt.system, resource.definition.prompt.system);
    assert_eq!(prompt.tools, vec!["search", "get"]);
    assert_eq!(
        registry
            .find("legacy")
            .unwrap()
            .resolve(serde_json::json!({}), &ctx)
            .await
            .unwrap()
            .system,
        "Legacy prompt"
    );
}

#[test]
fn cli_lists_shows_validates_and_previews_without_credentials_or_database() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    write(&root.join(".ctx/config.toml"), CONFIG);
    write(&root.join(".ctx/agents/researcher.toml"), RESOURCE);
    let entries: Value =
        serde_json::from_str(&success(run(&root, &global, &["agent", "list", "--json"]))).unwrap();
    assert_eq!(entries[0]["name"], "researcher");
    let shown: Value = serde_json::from_str(&success(run(
        &root,
        &global,
        &["agent", "show", "researcher", "--json"],
    )))
    .unwrap();
    assert_eq!(shown["resource"]["scope"], "workspace");
    assert!(shown["resource"]["version"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    let human = success(run(&root, &global, &["agent", "show", "researcher"]));
    assert!(human.contains("Allowed capabilities: [\"read_only\"]"));
    assert!(success(run(&root, &global, &["agent", "validate"]))
        .contains("Validated 1 executable agents"));
    assert!(
        success(run(&root, &global, &["agent", "test", "researcher"]))
            .contains("Use project context")
    );
    assert!(!root.join(".ctx/data").exists());
    let missing = run(&root, &global, &["agent", "show", "missing"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not found"));
}

#[test]
fn runtime_control_resources_validate_offline_without_credentials_or_database() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    write(&root.join(".ctx/config.toml"), CONFIG);
    write(
        &root.join(".ctx/agents/researcher.toml"),
        &RESOURCE.replace(
            "tools = [\"search\", \"get\"]",
            "tools = [\"run.blocked\", \"run.request_user_input\"]",
        ),
    );

    assert!(success(run(&root, &global, &["agent", "validate"]))
        .contains("Validated 1 executable agents"));
    assert!(!root.join(".ctx/data").exists());
}

#[test]
fn cli_explicit_config_and_env_config_isolate_resources_and_models() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    write(&root.join("custom/settings.toml"), CONFIG);
    write(&root.join("custom/agents/researcher.toml"), RESOURCE);
    write(
        &root.join(".ctx/agents/bad.toml"),
        "INVALID WORKSPACE RESOURCE",
    );
    write(&global.join("agents/bad.toml"), "INVALID GLOBAL RESOURCE");
    let shown: Value = serde_json::from_str(&success(run(
        &root,
        &global,
        &[
            "--config",
            "custom/settings.toml",
            "agent",
            "show",
            "researcher",
            "--json",
        ],
    )))
    .unwrap();
    assert_eq!(shown["resource"]["scope"], "explicit");
    let output = Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(&root)
        .env("CTX_CONFIG_DIR", &global)
        .env("CTX_CONFIG", "custom/settings.toml")
        .args(["agent", "validate"])
        .output()
        .unwrap();
    assert!(success(output).contains("Validated 1 executable agents"));
}

#[test]
fn ollama_static_validation_and_inspection_are_offline_and_sanitized() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    let config = CONFIG
        .replace("provider = \"fake\"", "provider = \"ollama\"")
        .replace("model = \"test-model\"", "model = \"qwen3\"")
        .replace(
            "api_key_env = \"CTX_TEST_UNUSED_KEY\"",
            "base_url = \"http://127.0.0.1:9\"\ntimeout_seconds = 45",
        );
    write(&root.join(".ctx/config.toml"), &config);
    write(&root.join(".ctx/agents/researcher.toml"), RESOURCE);

    assert!(success(run(&root, &global, &["agent", "validate"]))
        .contains("Validated 1 executable agents"));
    let shown: Value = serde_json::from_str(&success(run(
        &root,
        &global,
        &["agent", "show", "researcher", "--json"],
    )))
    .unwrap();
    assert_eq!(shown["model_config"]["provider"], "ollama");
    assert_eq!(shown["model_config"]["model"], "qwen3");
    assert_eq!(shown["model_config"]["base_url"], "http://127.0.0.1:9");
    assert_eq!(shown["model_config"]["timeout_seconds"], 45);
    assert_eq!(shown["model_config"]["local_only"], true);
    assert!(!root.join(".ctx/data").exists());
}

#[test]
fn cli_global_defaults_and_workspace_override_are_consistent() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    write(&global.join("config.toml"), CONFIG);
    write(&global.join("agents/researcher.toml"), RESOURCE);
    write(
        &root.join(".ctx/config.toml"),
        "[models.reasoning]\nmodel = 'workspace-model'",
    );
    write(
        &root.join(".ctx/agents/researcher.toml"),
        &RESOURCE
            .replace("name =", "override = true\nname =")
            .replace("Use project context", "Local instructions"),
    );
    let shown: Value = serde_json::from_str(&success(run(
        &root,
        &global,
        &["agent", "show", "researcher", "--json"],
    )))
    .unwrap();
    assert_eq!(shown["resource"]["scope"], "workspace");
    assert!(shown["system_prompt"]
        .as_str()
        .unwrap()
        .starts_with("Local instructions"));
}

#[test]
fn malformed_resources_fail_agent_validation_but_do_not_break_unrelated_commands() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    write(&root.join(".ctx/config.toml"), CONFIG);
    write(
        &root.join(".ctx/agents/bad.toml"),
        &RESOURCE.replace("tools =", "toools ="),
    );
    let output = run(&root, &global, &["agent", "validate"]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("bad.toml") && error.contains("toools"),
        "{error}"
    );
    success(run(&root, &global, &["sources"]));
}

#[test]
fn testing_one_legacy_agent_does_not_load_unrelated_broken_definitions() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    write(&root.join("working.lua"), "agent = {name = 'working', tools = {}}\nfunction agent.resolve(args, config, context) return {system = 'Working Lua prompt', tools = {}} end");
    write(&root.join(".ctx/config.toml"), &format!("{CONFIG}\n[agents.script.working]\npath = 'working.lua'\n[agents.script.broken]\npath = 'missing.lua'\n[agents.inline.static]\ndescription = 'static'\ntools = []\nsystem_prompt = 'Working inline prompt'"));
    write(&root.join(".ctx/agents/broken.toml"), "INVALID RESOURCE");
    assert!(
        success(run(&root, &global, &["agent", "test", "working"])).contains("Working Lua prompt")
    );
    assert!(success(run(&root, &global, &["agent", "test", "static"]))
        .contains("Working inline prompt"));
}
