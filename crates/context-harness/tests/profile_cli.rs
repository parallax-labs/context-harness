use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn profile_and_agent_commands_have_distinct_catalogs() {
    let tmp = TempDir::new().unwrap();
    let ctx = tmp.path().join(".ctx");
    fs::create_dir_all(ctx.join("agents")).unwrap();
    fs::write(
        ctx.join("config.toml"),
        r#"
[db]
path = "unused.sqlite"
[chunking]
max_tokens = 700
[retrieval]
final_limit = 12
[server]
bind = "127.0.0.1:0"
[models.test]
provider = "openai"
model = "unused"
api_key_env = "CTX_PROFILE_CLI_UNUSED"
[profiles.inline.reviewer]
description = "Review profile"
tools = ["search"]
system_prompt = "Review against project conventions."
"#,
    )
    .unwrap();
    fs::write(
        ctx.join("agents/researcher.toml"),
        r#"
[agent]
name = "researcher"
description = "Executable researcher"
model = "test"
tools = []
[prompt]
system = "Research the project."
"#,
    )
    .unwrap();

    let profiles = run(tmp.path(), &["profile", "list"]);
    assert!(profiles.status.success(), "{:?}", profiles);
    let profiles = String::from_utf8(profiles.stdout).unwrap();
    assert!(profiles.contains("reviewer"));
    assert!(profiles.contains("researcher"));

    let agents = run(tmp.path(), &["agent", "list"]);
    assert!(agents.status.success(), "{:?}", agents);
    let agents = String::from_utf8(agents.stdout).unwrap();
    assert!(agents.contains("researcher"));
    assert!(!agents.contains("reviewer"));
}

#[test]
fn historical_agents_config_is_still_a_profile_alias() {
    let tmp = TempDir::new().unwrap();
    let ctx = tmp.path().join(".ctx");
    fs::create_dir_all(&ctx).unwrap();
    fs::write(
        ctx.join("config.toml"),
        r#"
[db]
path = "unused.sqlite"
[chunking]
max_tokens = 700
[retrieval]
final_limit = 12
[server]
bind = "127.0.0.1:0"
[agents.inline.compat]
description = "Compatibility profile"
tools = []
system_prompt = "Still works."
"#,
    )
    .unwrap();

    let output = run(tmp.path(), &["profile", "list"]);
    assert!(output.status.success(), "{:?}", output);
    assert!(String::from_utf8(output.stdout).unwrap().contains("compat"));
}
