use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
fn setup(root: &Path, tools: &str) {
    write(&root.join(".ctx/config.toml"), "[db]\npath='.ctx/data/ctx.sqlite'\n[chunking]\nmax_tokens=700\n[retrieval]\nfinal_limit=12\n[server]\nbind='127.0.0.1:0'\n[models.test]\nprovider='fake'\nmodel='test'\n");
    write(&root.join(".ctx/agents/researcher.toml"), &format!("[agent]\nname='researcher'\nmodel='test'\ntools={tools}\n[prompt]\nsystem='Use context.'\n"));
}
fn run(root: &Path, global: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ctx"))
        .current_dir(root)
        .env("CTX_CONFIG_DIR", global)
        .env_remove("CTX_CONFIG")
        .args(args)
        .output()
        .unwrap()
}
fn json(output: Output, successful: bool) -> Value {
    assert_eq!(
        output.status.success(),
        successful,
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn cli_run_history_and_paged_inspect_work_without_credentials() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    setup(&root, "[]");
    let completed = json(
        run(
            &root,
            &global,
            &[
                "agent",
                "run",
                "researcher",
                "Question",
                "--json",
                "--non-interactive",
            ],
        ),
        true,
    );
    assert_eq!(completed["status"], "completed");
    assert!(completed["output"]
        .as_str()
        .unwrap()
        .contains("Synthetic response"));
    let id = completed["id"].as_str().unwrap();
    // History/inspect do not depend on still-valid resource files or providers.
    write(&root.join(".ctx/agents/researcher.toml"), "BROKEN RESOURCE");
    let history = json(run(&root, &global, &["agent", "history", "--json"]), true);
    assert_eq!(history[0]["id"], id);
    let inspected = json(
        run(
            &root,
            &global,
            &["agent", "inspect", id, "--json", "--limit", "2"],
        ),
        true,
    );
    assert_eq!(inspected["events"].as_array().unwrap().len(), 2);
    assert_eq!(inspected["next_after_sequence"], 2);
    let page2 = json(
        run(
            &root,
            &global,
            &["agent", "inspect", id, "--json", "--after-sequence", "2"],
        ),
        true,
    );
    assert_eq!(page2["events"][0]["event_type"], "checkpoint.created");
    assert_eq!(
        page2["events"].as_array().unwrap().last().unwrap()["event_type"],
        "run.completed"
    );
    let human = run(&root, &global, &["agent", "inspect", id]);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("run.completed"));
}
#[test]
fn failed_run_returns_nonzero_with_inspectable_json_record() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    setup(&root, "['process.exec']");
    let failed = json(
        run(
            &root,
            &global,
            &["agent", "run", "researcher", "Question", "--json"],
        ),
        false,
    );
    assert_eq!(failed["status"], "failed");
    let id = failed["id"].as_str().unwrap();
    let inspected = json(
        run(&root, &global, &["agent", "inspect", id, "--json"]),
        true,
    );
    assert_eq!(inspected["events"].as_array().unwrap().len(), 2);
    assert_eq!(inspected["events"][1]["event_type"], "run.failed");
}
#[test]
fn history_is_read_only_and_run_ids_cannot_cross_workspace_roots() {
    let tmp = TempDir::new().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let global = tmp.path().join("global");
    setup(&a, "[]");
    setup(&b, "[]");
    assert_eq!(
        json(run(&a, &global, &["agent", "history", "--json"]), true),
        serde_json::json!([])
    );
    assert!(!a.join(".ctx/data").exists());
    let completed = json(
        run(
            &a,
            &global,
            &["agent", "run", "researcher", "Question", "--json"],
        ),
        true,
    );
    let id = completed["id"].as_str().unwrap();
    let shared_config = a.join(".ctx/config.toml");
    // Explicitly point both roots at the same absolute DB to test logical scope.
    let text = fs::read_to_string(&shared_config).unwrap().replace(
        "'.ctx/data/ctx.sqlite'",
        &format!("{:?}", a.join(".ctx/data/ctx.sqlite").to_str().unwrap()),
    );
    write(&shared_config, &text);
    let result = run(
        &b,
        &global,
        &[
            "--config",
            shared_config.to_str().unwrap(),
            "agent",
            "inspect",
            id,
        ],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("not found in this workspace"));
}
