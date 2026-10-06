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

fn setup(root: &Path) {
    write(
        &root.join(".ctx/config.toml"),
        "[db]\npath='.ctx/data/ctx.sqlite'\n[chunking]\nmax_tokens=700\n[retrieval]\nfinal_limit=12\n[server]\nbind='127.0.0.1:0'\n[models.test]\nprovider='fake'\nmodel='test'\n",
    );
    write(
        &root.join(".ctx/agents/researcher.toml"),
        "[agent]\nname='researcher'\nmodel='test'\ntools=[]\n[prompt]\nsystem='Use context.'\n",
    );
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

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn enqueue_list_inspect_and_cancel_are_credential_safe() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    setup(&root);

    let created = run(
        &root,
        &global,
        &[
            "agent",
            "enqueue",
            "researcher",
            "Question",
            "--request-key",
            "request-1",
            "--json",
        ],
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let created_json = json(&created);
    assert_eq!(created_json["disposition"], "created");
    assert!(!String::from_utf8_lossy(&created.stdout).contains("claim_token"));
    let id = created_json["task"]["id"].as_str().unwrap();

    // Job inspection and cancellation use persisted state only. They must not
    // load resources or construct the now-unknown provider.
    let config = fs::read_to_string(root.join(".ctx/config.toml"))
        .unwrap()
        .replace("provider='fake'", "provider='unregistered'");
    write(&root.join(".ctx/config.toml"), &config);
    write(&root.join(".ctx/agents/researcher.toml"), "BROKEN RESOURCE");

    let jobs = run(&root, &global, &["agent", "jobs", "--json"]);
    assert!(jobs.status.success());
    assert_eq!(json(&jobs)[0]["task"]["id"], id);
    assert!(!String::from_utf8_lossy(&jobs.stdout).contains("claim_token"));

    let inspected = run(
        &root,
        &global,
        &["agent", "job", "inspect", id, "--json", "--limit", "1"],
    );
    assert!(inspected.status.success());
    let inspected_json = json(&inspected);
    assert_eq!(inspected_json["task"]["id"], id);
    assert_eq!(inspected_json["events"][0]["event_type"], "task.submitted");
    assert!(!String::from_utf8_lossy(&inspected.stdout).contains("claim_token"));

    let cancelled = run(&root, &global, &["agent", "job", "cancel", id, "--json"]);
    assert!(cancelled.status.success());
    let cancelled_json = json(&cancelled);
    assert_eq!(cancelled_json["status"], "terminal");
    assert_eq!(cancelled_json["scheduling_reason"], "cancelled_before_run");
    assert!(!String::from_utf8_lossy(&cancelled.stdout).contains("claim_token"));
}

#[test]
fn enqueue_is_idempotent_and_reports_conflicts_and_queue_limits() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    setup(&root);
    let base = [
        "agent",
        "enqueue",
        "researcher",
        "Question",
        "--request-key",
        "same-key",
        "--queue-limit",
        "2",
        "--json",
    ];
    let first = run(&root, &global, &base);
    assert!(first.status.success());
    let existing = run(&root, &global, &base);
    assert!(existing.status.success());
    assert_eq!(json(&existing)["disposition"], "existing_identical");

    let conflict = run(
        &root,
        &global,
        &[
            "agent",
            "enqueue",
            "researcher",
            "Different",
            "--request-key",
            "same-key",
            "--queue-limit",
            "2",
            "--json",
        ],
    );
    assert!(!conflict.status.success());
    assert_eq!(json(&conflict)["disposition"], "request_conflict");

    let full = run(
        &root,
        &global,
        &[
            "agent",
            "enqueue",
            "researcher",
            "Another",
            "--request-key",
            "another-key",
            "--queue-limit",
            "1",
            "--json",
        ],
    );
    assert!(!full.status.success());
    assert_eq!(json(&full)["disposition"], "queue_full");
}

#[test]
fn job_reads_do_not_create_the_database_and_worker_options_are_bounded() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("project");
    let global = tmp.path().join("global");
    setup(&root);

    let jobs = run(&root, &global, &["agent", "jobs", "--json"]);
    assert!(jobs.status.success());
    assert_eq!(json(&jobs), serde_json::json!([]));
    assert!(!root.join(".ctx/data").exists());

    let missing = run(
        &root,
        &global,
        &["agent", "job", "inspect", "missing", "--json"],
    );
    assert!(!missing.status.success());
    assert!(!root.join(".ctx/data").exists());

    let invalid = run(
        &root,
        &global,
        &["agent", "worker", "--max-concurrency", "0"],
    );
    assert!(!invalid.status.success());
    assert!(!root.join(".ctx/data").exists());
}
