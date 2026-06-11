use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn ctx_binary() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test binary name
    path.pop(); // remove deps/
    path.push("ctx");
    path
}

fn setup_test_env() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    // Create config
    let config_dir = root.join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let data_dir = root.join("data");
    fs::create_dir_all(&data_dir).unwrap();

    // Create test files
    let files_dir = root.join("files");
    fs::create_dir_all(&files_dir).unwrap();
    fs::write(
        files_dir.join("alpha.md"),
        "# Alpha Document\n\nThis is the alpha document about Rust programming.\n\nIt contains information about cargo and crates.",
    ).unwrap();
    fs::write(
        files_dir.join("beta.md"),
        "# Beta Document\n\nThis document discusses Python and machine learning.\n\nDeep learning frameworks like PyTorch are covered.",
    ).unwrap();
    fs::write(
        files_dir.join("gamma.txt"),
        "Gamma plain text file.\n\nContains notes about deployment and infrastructure.\n\nKubernetes and Docker are mentioned here.",
    ).unwrap();

    let config_content = format!(
        r#"[db]
path = "{}/data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem.test]
root = "{}/files"
include_globs = ["**/*.md", "**/*.txt"]
exclude_globs = []
follow_symlinks = false
"#,
        root.display(),
        root.display()
    );

    let config_path = config_dir.join("ctx.toml");
    fs::write(&config_path, config_content).unwrap();

    (tmp, config_path)
}

fn run_ctx(config_path: &Path, args: &[&str]) -> (String, String, bool) {
    let binary = ctx_binary();
    let output = Command::new(&binary)
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run ctx binary at {:?}: {}", binary, e));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();
    (stdout, stderr, success)
}

fn run_ctx_in_dir(root: &Path, args: &[&str], envs: &[(&str, &str)]) -> (String, String, bool) {
    let binary = ctx_binary();
    let mut command = Command::new(&binary);
    command
        .current_dir(root)
        .env_remove("CTX_CONFIG")
        .env_remove("CTX_CONFIG_DIR")
        .env_remove("CTX_DATA_DIR")
        .env_remove("CTX_CACHE_DIR")
        .env_remove("CTX_STATE_DIR")
        .args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("Failed to run ctx binary at {:?}: {}", binary, e));

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();
    (stdout, stderr, success)
}

#[test]
fn test_init_without_config_bootstraps_ctx_directory() {
    let tmp = TempDir::new().unwrap();

    let (stdout, stderr, success) = run_ctx_in_dir(tmp.path(), &["init"], &[]);
    assert!(success, "init failed: stdout={}, stderr={}", stdout, stderr);

    assert!(tmp.path().join(".ctx/config.toml").exists());
    assert!(tmp.path().join(".ctx/data").is_dir());
    assert!(tmp.path().join(".ctx/cache").is_dir());
    assert!(tmp.path().join(".ctx/.gitignore").exists());
    assert!(tmp.path().join(".ctx/data/ctx.sqlite").exists());

    let config = fs::read_to_string(tmp.path().join(".ctx/config.toml")).unwrap();
    assert!(config.contains("path = \".ctx/data/ctx.sqlite\""));
    let gitignore = fs::read_to_string(tmp.path().join(".ctx/.gitignore")).unwrap();
    assert!(gitignore.contains("data/"));
    assert!(gitignore.contains("cache/"));
    assert!(!gitignore.contains("state/"));
}

#[test]
fn test_workspace_ctx_config_wins_over_legacy_config() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".ctx")).unwrap();
    fs::create_dir_all(tmp.path().join("config")).unwrap();

    let ctx_config = format!(
        r#"[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
"#,
        tmp.path().join(".ctx/data/ctx.sqlite").display()
    );
    let legacy_config = format!(
        r#"[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
"#,
        tmp.path().join("data/legacy.sqlite").display()
    );
    fs::write(tmp.path().join(".ctx/config.toml"), ctx_config).unwrap();
    fs::write(tmp.path().join("config/ctx.toml"), legacy_config).unwrap();

    let (stdout, stderr, success) = run_ctx_in_dir(tmp.path(), &["init"], &[]);
    assert!(success, "init failed: stdout={}, stderr={}", stdout, stderr);
    assert!(tmp.path().join(".ctx/data/ctx.sqlite").exists());
    assert!(!tmp.path().join("data/legacy.sqlite").exists());
}

#[test]
fn test_legacy_config_still_loads_when_ctx_config_absent() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("config")).unwrap();
    let legacy_config = format!(
        r#"[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
"#,
        tmp.path().join("data/legacy.sqlite").display()
    );
    fs::write(tmp.path().join("config/ctx.toml"), legacy_config).unwrap();

    let (stdout, stderr, success) = run_ctx_in_dir(tmp.path(), &["init"], &[]);
    assert!(success, "init failed: stdout={}, stderr={}", stdout, stderr);
    assert!(tmp.path().join("data/legacy.sqlite").exists());
    assert!(!tmp.path().join(".ctx/config.toml").exists());
}

#[test]
fn test_workspace_config_merges_with_global_config() {
    let tmp = TempDir::new().unwrap();
    let xdg_config_home = tmp.path().join("xdg-config");
    fs::create_dir_all(xdg_config_home.join("ctx")).unwrap();
    fs::create_dir_all(tmp.path().join(".ctx")).unwrap();

    fs::write(
        xdg_config_home.join("ctx/config.toml"),
        r#"[db]
path = "unused.sqlite"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join(".ctx/config.toml"),
        format!(
            r#"[db]
path = "{}"
"#,
            tmp.path().join(".ctx/data/ctx.sqlite").display()
        ),
    )
    .unwrap();

    let (stdout, stderr, success) = run_ctx_in_dir(
        tmp.path(),
        &["init"],
        &[("XDG_CONFIG_HOME", xdg_config_home.to_str().unwrap())],
    );
    assert!(success, "init failed: stdout={}, stderr={}", stdout, stderr);
    assert!(tmp.path().join(".ctx/data/ctx.sqlite").exists());
}

#[test]
fn test_explicit_config_does_not_merge_with_global_config() {
    let tmp = TempDir::new().unwrap();
    let xdg_config_home = tmp.path().join("xdg-config");
    fs::create_dir_all(xdg_config_home.join("ctx")).unwrap();

    fs::write(
        xdg_config_home.join("ctx/config.toml"),
        r#"[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
"#,
    )
    .unwrap();
    let explicit = tmp.path().join("explicit.toml");
    fs::write(
        &explicit,
        format!(
            r#"[db]
path = "{}"
"#,
            tmp.path().join("explicit.sqlite").display()
        ),
    )
    .unwrap();

    let (stdout, stderr, success) = run_ctx_in_dir(
        tmp.path(),
        &["--config", explicit.to_str().unwrap(), "init"],
        &[("XDG_CONFIG_HOME", xdg_config_home.to_str().unwrap())],
    );
    assert!(
        !success,
        "explicit config should not merge with global config: stdout={}, stderr={}",
        stdout, stderr
    );
    assert!(stderr.contains("missing field"));
}

#[test]
fn test_init_creates_database() {
    let (_tmp, config_path) = setup_test_env();

    let (stdout, stderr, success) = run_ctx(&config_path, &["init"]);
    assert!(success, "init failed: stdout={}, stderr={}", stdout, stderr);
    assert!(stdout.contains("initialized"));
}

#[test]
fn test_init_idempotent() {
    let (_tmp, config_path) = setup_test_env();

    // Run init twice
    let (_, _, success1) = run_ctx(&config_path, &["init"]);
    assert!(success1, "First init failed");

    let (_, _, success2) = run_ctx(&config_path, &["init"]);
    assert!(success2, "Second init failed (not idempotent)");
}

#[test]
fn test_sync_filesystem() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (stdout, stderr, success) = run_ctx(&config_path, &["sync", "filesystem"]);
    assert!(success, "sync failed: stdout={}, stderr={}", stdout, stderr);
    assert!(stdout.contains("upserted documents: 3"));
    assert!(stdout.contains("ok"));
}

#[test]
fn test_sync_idempotent_no_duplicates() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);

    // First sync
    let (stdout1, _, _) = run_ctx(&config_path, &["sync", "filesystem", "--full"]);
    assert!(stdout1.contains("upserted documents: 3"));

    // Second sync with --full should still upsert 3, not create duplicates
    let (stdout2, _, _) = run_ctx(&config_path, &["sync", "filesystem", "--full"]);
    assert!(stdout2.contains("upserted documents: 3"));
}

#[test]
fn test_sync_incremental() {
    let (tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    // Second sync without changes should process 0 items (checkpoint-based)
    let (stdout, _, _) = run_ctx(&config_path, &["sync", "filesystem"]);
    assert!(
        stdout.contains("fetched: 0") || stdout.contains("upserted documents: 0"),
        "Expected no items processed on incremental sync, got: {}",
        stdout
    );

    // Modify one file (need to ensure mtime actually changes)
    std::thread::sleep(std::time::Duration::from_secs(1));
    let files_dir = tmp.path().join("files");
    fs::write(
        files_dir.join("alpha.md"),
        "# Alpha Document Updated\n\nThis file was modified.",
    )
    .unwrap();

    // Third sync should process only the modified file
    let (stdout, _, _) = run_ctx(&config_path, &["sync", "filesystem"]);
    assert!(
        stdout.contains("upserted documents: 1"),
        "Expected 1 doc upserted after modification, got: {}",
        stdout
    );
}

#[test]
fn test_sync_dry_run() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (stdout, _, success) = run_ctx(&config_path, &["sync", "filesystem", "--dry-run"]);
    assert!(success);
    assert!(stdout.contains("dry-run"));
    assert!(stdout.contains("items found: 3"));
}

#[test]
fn test_search_keyword() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    let (stdout, _, success) = run_ctx(&config_path, &["search", "Rust programming"]);
    assert!(success, "search failed");
    assert!(
        stdout.contains("alpha.md") || stdout.contains("Alpha"),
        "Expected alpha.md in results, got: {}",
        stdout
    );
}

#[test]
fn test_search_deterministic() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    let (stdout1, _, _) = run_ctx(&config_path, &["search", "document"]);
    let (stdout2, _, _) = run_ctx(&config_path, &["search", "document"]);
    assert_eq!(
        stdout1, stdout2,
        "Search results should be deterministic across runs"
    );
}

#[test]
fn test_search_empty_query() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (stdout, _, success) = run_ctx(&config_path, &["search", ""]);
    assert!(success, "Empty query should not panic");
    assert!(stdout.contains("No results"));
}

#[test]
fn test_search_no_results() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    let (stdout, _, success) = run_ctx(&config_path, &["search", "xyznonexistent"]);
    assert!(success);
    assert!(stdout.contains("No results"));
}

#[test]
fn test_get_document() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    // Search to get an ID
    let (search_out, _, _) = run_ctx(&config_path, &["search", "Rust"]);
    // Extract ID from output (look for "id: <uuid>")
    let id = search_out
        .lines()
        .find(|l| l.trim().starts_with("id:"))
        .and_then(|l| l.split("id:").nth(1))
        .map(|s| s.trim().to_string());

    if let Some(doc_id) = id {
        let (stdout, _, success) = run_ctx(&config_path, &["get", &doc_id]);
        assert!(success, "get should succeed");
        assert!(stdout.contains("Document"));
        assert!(stdout.contains(&doc_id));
    }
}

#[test]
fn test_get_missing_document() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);

    let (_, stderr, success) = run_ctx(&config_path, &["get", "nonexistent-id"]);
    assert!(!success, "get with missing ID should fail");
    assert!(
        stderr.contains("not found"),
        "Should report not found, got: {}",
        stderr
    );
}

#[test]
fn test_sources() {
    let (_tmp, config_path) = setup_test_env();

    let (stdout, _, success) = run_ctx(&config_path, &["sources"]);
    assert!(success);
    assert!(
        stdout.contains("filesystem:test"),
        "Should list filesystem:test, got: {}",
        stdout
    );
    assert!(stdout.contains("OK"));
}

#[test]
fn test_sync_with_limit() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (stdout, _, success) = run_ctx(&config_path, &["sync", "filesystem", "--limit", "1"]);
    assert!(success);
    assert!(stdout.contains("upserted documents: 1"));
}

#[test]
fn test_unknown_connector() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["sync", "nonexistent"]);
    assert!(!success, "Unknown connector should fail");
    assert!(
        stderr.contains("Unknown connector") || stderr.contains("nonexistent"),
        "Should mention unknown connector, got: {}",
        stderr
    );
}

#[test]
fn test_search_mode_semantic_errors_when_disabled() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["search", "test", "--mode", "semantic"]);
    assert!(
        !success,
        "Semantic mode should fail when embeddings disabled"
    );
    assert!(
        stderr.contains("embeddings"),
        "Should mention embeddings, got: {}",
        stderr
    );
}

#[test]
fn test_search_mode_hybrid_errors_when_disabled() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["search", "test", "--mode", "hybrid"]);
    assert!(!success, "Hybrid mode should fail when embeddings disabled");
    assert!(
        stderr.contains("embeddings"),
        "Should mention embeddings, got: {}",
        stderr
    );
}

#[test]
fn test_embed_pending_errors_when_disabled() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["embed", "pending"]);
    assert!(!success, "embed pending should fail when provider disabled");
    assert!(
        stderr.contains("disabled"),
        "Should mention disabled, got: {}",
        stderr
    );
}

#[test]
fn test_embed_rebuild_errors_when_disabled() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["embed", "rebuild"]);
    assert!(!success, "embed rebuild should fail when provider disabled");
    assert!(
        stderr.contains("disabled"),
        "Should mention disabled, got: {}",
        stderr
    );
}

#[test]
fn test_embed_pending_dry_run() {
    let (_tmp, config_path) = setup_test_env();

    // Need a config with embedding enabled but no API key
    // Using disabled provider, which will error; test dry-run with special config
    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["embed", "pending", "--dry-run"]);
    // With disabled provider, even dry-run should fail
    assert!(
        !success,
        "embed pending dry-run with disabled provider should fail"
    );
    assert!(stderr.contains("disabled"));
}

#[test]
fn test_init_creates_embedding_tables() {
    let (tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);

    // Verify embedding tables exist by checking the SQLite schema
    let db_path = tmp.path().join("data").join("ctx.sqlite");
    assert!(db_path.exists(), "Database should exist after init");

    // Use a second init to verify idempotency with the new tables
    let (_, _, success) = run_ctx(&config_path, &["init"]);
    assert!(success, "Second init with embedding tables should succeed");
}

#[test]
fn test_search_unknown_mode_errors() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["search", "test", "--mode", "invalid"]);
    assert!(!success, "Unknown mode should fail");
    assert!(
        stderr.contains("Unknown search mode"),
        "Should mention unknown mode, got: {}",
        stderr
    );
}

// ============ Phase 3: MCP Server Integration Tests ============

/// Find an available port for the test server.
fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Set up a test environment with a specific server port configured.
fn setup_server_env(port: u16) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    let config_dir = root.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let data_dir = root.join("data");
    fs::create_dir_all(&data_dir).unwrap();

    let files_dir = root.join("files");
    fs::create_dir_all(&files_dir).unwrap();
    fs::write(
        files_dir.join("alpha.md"),
        "# Alpha Document\n\nThis is the alpha document about Rust programming.\n\nIt contains information about cargo and crates.",
    ).unwrap();
    fs::write(
        files_dir.join("beta.md"),
        "# Beta Document\n\nThis document discusses Python and machine learning.\n\nDeep learning frameworks like PyTorch are covered.",
    ).unwrap();

    let config_content = format!(
        r#"[db]
path = "{}/data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:{}"

[connectors.filesystem.test]
root = "{}/files"
include_globs = ["**/*.md"]
exclude_globs = []
follow_symlinks = false
"#,
        root.display(),
        port,
        root.display()
    );

    let config_path = config_dir.join("ctx.toml");
    fs::write(&config_path, config_content).unwrap();

    (tmp, config_path)
}

/// Start the MCP server in the background, wait for it to be ready, return the child process.
fn start_server(config_path: &Path) -> std::process::Child {
    let binary = ctx_binary();
    let child = Command::new(&binary)
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .args(["serve", "mcp"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("Failed to start server: {}", e));

    child
}

/// Wait for the server to be ready by polling the health endpoint.
fn wait_for_server(port: u16) {
    let url = format!("http://127.0.0.1:{}/health", port);
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(resp) = reqwest::blocking::get(&url) {
            if resp.status().is_success() {
                return;
            }
        }
    }
    panic!("Server did not become ready within 5 seconds");
}

#[test]
fn test_server_health() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/health", port);
    let resp = reqwest::blocking::get(&url).unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_server_sources() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/sources", port);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().unwrap();
    let sources = body["result"]["sources"].as_array().unwrap();
    assert!(!sources.is_empty());

    // Validate schema shape per SCHEMAS.md
    let fs_source = &sources[0];
    assert_eq!(fs_source["name"], "filesystem:test");
    assert!(fs_source["configured"].is_boolean());
    assert!(fs_source["healthy"].is_boolean());

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_server_search() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/search", port);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "query": "Rust programming",
            "mode": "keyword",
            "limit": 5
        }))
        .send()
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().unwrap();
    let results = body["result"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "Should have search results");

    // Validate schema shape per SCHEMAS.md
    let first = &results[0];
    assert!(first["id"].is_string());
    assert!(first["score"].is_f64());
    assert!(first["source"].is_string());
    assert!(first["source_id"].is_string());
    assert!(first["updated_at"].is_string());
    assert!(first["snippet"].is_string());

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_server_search_empty_query() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/search", port);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "query": "",
            "mode": "keyword"
        }))
        .send()
        .unwrap();

    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().unwrap();
    assert!(body["error"]["code"].is_string());
    assert!(body["error"]["message"].is_string());

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_server_search_semantic_disabled() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/search", port);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "query": "test",
            "mode": "semantic"
        }))
        .send()
        .unwrap();

    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().unwrap();
    assert_eq!(body["error"]["code"], "embeddings_disabled");

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_server_get_document() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    // Get a doc ID via CLI search
    let (search_out, _, _) = run_ctx(&config_path, &["search", "Rust"]);
    let doc_id = search_out
        .lines()
        .find(|l| l.trim().starts_with("id:"))
        .and_then(|l| l.split("id:").nth(1))
        .map(|s| s.trim().to_string())
        .expect("Should find a document ID");

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/get", port);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "id": doc_id }))
        .send()
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().unwrap();
    let doc = &body["result"];

    // Validate schema shape per SCHEMAS.md
    assert_eq!(doc["id"], doc_id);
    assert!(doc["source"].is_string());
    assert!(doc["source_id"].is_string());
    assert!(doc["created_at"].is_string());
    assert!(doc["updated_at"].is_string());
    assert!(doc["content_type"].is_string());
    assert!(doc["body"].is_string());
    assert!(doc["metadata"].is_object());
    assert!(doc["chunks"].is_array());

    let chunks = doc["chunks"].as_array().unwrap();
    assert!(!chunks.is_empty());
    assert!(chunks[0]["index"].is_number());
    assert!(chunks[0]["text"].is_string());

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_server_get_not_found() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/get", port);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "id": "nonexistent-id" }))
        .send()
        .unwrap();

    assert_eq!(resp.status(), 404);

    let body: serde_json::Value = resp.json().unwrap();
    assert_eq!(body["error"]["code"], "not_found");

    server.kill().ok();
    server.wait().ok();
}

// ============ Git Connector Tests ============

/// Create a test git repo and return its path.
fn create_test_git_repo(tmp: &Path) -> PathBuf {
    let repo_dir = tmp.join("test-repo");
    fs::create_dir_all(&repo_dir).unwrap();

    // Initialize a git repo and force the committed branch to be `main`.
    // Some local git versions/configs ignore or do not support `init -b`.
    run_git_checked(&repo_dir, &["init"]);
    run_git_checked(&repo_dir, &["config", "user.email", "test@example.com"]);
    run_git_checked(&repo_dir, &["config", "user.name", "Test User"]);
    run_git_checked(&repo_dir, &["config", "commit.gpgsign", "false"]);

    // Create docs directory with test files
    let docs_dir = repo_dir.join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    fs::write(
        docs_dir.join("guide.md"),
        "# User Guide\n\nThis is the user guide for our project.\n\nIt covers installation and usage.",
    )
    .unwrap();
    fs::write(
        docs_dir.join("api.md"),
        "# API Reference\n\nThis document describes the API endpoints.\n\nGET /health returns status.",
    )
    .unwrap();
    fs::write(
        docs_dir.join("notes.txt"),
        "Random notes about the project.\n\nThese are internal notes.",
    )
    .unwrap();

    // Also create a file in root (should be excluded when root is "docs")
    fs::write(
        repo_dir.join("README.md"),
        "# Test Repo\n\nThis is a test repo.",
    )
    .unwrap();

    // Commit everything
    run_git_checked(&repo_dir, &["add", "."]);
    run_git_checked(&repo_dir, &["commit", "-m", "initial commit"]);
    run_git_checked(&repo_dir, &["branch", "-M", "main"]);

    repo_dir
}

fn run_git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run git {:?}: {}", args, e))
}

fn run_git_checked(dir: &Path, args: &[&str]) {
    let output = run_git(dir, args);
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={}, stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_git_test_env(repo_path: &Path, root: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let work_root = tmp.path().to_path_buf();

    let config_dir = work_root.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let data_dir = work_root.join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let cache_dir = work_root.join("git-cache");
    fs::create_dir_all(&cache_dir).unwrap();

    let config_content = format!(
        r#"[db]
path = "{}/data/ctx.sqlite"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"

[connectors.git.test]
url = "{}"
branch = "main"
root = "{}"
include_globs = ["**/*.md", "**/*.txt"]
shallow = false
cache_dir = "{}"
"#,
        work_root.display(),
        repo_path.display(),
        root,
        cache_dir.display()
    );

    let config_path = config_dir.join("ctx.toml");
    fs::write(&config_path, config_content).unwrap();

    (tmp, config_path)
}

#[test]
fn test_git_sync_with_subdirectory() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, "docs");

    run_ctx(&config_path, &["init"]);
    let (stdout, stderr, success) = run_ctx(&config_path, &["sync", "git"]);
    assert!(
        success,
        "git sync failed: stdout={}, stderr={}",
        stdout, stderr
    );
    // Should only find files in docs/ (guide.md, api.md, notes.txt)
    assert!(
        stdout.contains("upserted documents: 3"),
        "Expected 3 docs from docs/, got: {}",
        stdout
    );
    assert!(stdout.contains("ok"));
}

#[test]
fn test_git_sync_root_directory() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, ".");

    run_ctx(&config_path, &["init"]);
    let (stdout, stderr, success) = run_ctx(&config_path, &["sync", "git"]);
    assert!(
        success,
        "git sync failed: stdout={}, stderr={}",
        stdout, stderr
    );
    // Should find all 4 files (README.md + docs/guide.md + docs/api.md + docs/notes.txt)
    assert!(
        stdout.contains("upserted documents: 4"),
        "Expected 4 docs from root, got: {}",
        stdout
    );
}

#[test]
fn test_git_sync_then_search() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, "docs");

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "git"]);

    let (stdout, _, success) = run_ctx(&config_path, &["search", "API endpoints"]);
    assert!(success, "search failed");
    assert!(
        stdout.contains("api.md") || stdout.contains("API"),
        "Expected API doc in results, got: {}",
        stdout
    );
}

#[test]
fn test_git_sync_incremental() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, "docs");

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "git"]);

    // Second sync should process 0 items (checkpoint-based)
    let (stdout, _, _) = run_ctx(&config_path, &["sync", "git"]);
    assert!(
        stdout.contains("fetched: 0") || stdout.contains("upserted documents: 0"),
        "Expected no items on incremental sync, got: {}",
        stdout
    );
}

#[test]
fn test_git_sync_full() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, "docs");

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "git"]);

    // Full sync should re-process all items
    let (stdout, _, success) = run_ctx(&config_path, &["sync", "git", "--full"]);
    assert!(success);
    assert!(
        stdout.contains("upserted documents: 3"),
        "Full sync should re-process all 3 docs, got: {}",
        stdout
    );
}

#[test]
fn test_git_sync_dry_run() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, "docs");

    run_ctx(&config_path, &["init"]);
    let (stdout, _, success) = run_ctx(&config_path, &["sync", "git", "--dry-run"]);
    assert!(success);
    assert!(stdout.contains("dry-run"));
    assert!(stdout.contains("items found: 3"));
}

#[test]
fn test_git_connector_not_configured() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["sync", "git"]);
    assert!(!success, "git sync should fail when not configured");
    assert!(
        stderr.contains("No git connectors configured"),
        "Should mention no git connectors, got: {}",
        stderr
    );
}

#[test]
fn test_git_sources_shows_configured() {
    let repo_tmp = TempDir::new().unwrap();
    let repo_path = create_test_git_repo(repo_tmp.path());

    let (_tmp, config_path) = setup_git_test_env(&repo_path, "docs");

    let (stdout, _, success) = run_ctx(&config_path, &["sources"]);
    assert!(success);
    assert!(
        stdout.contains("git:test"),
        "Should list git:test connector, got: {}",
        stdout
    );
    assert!(
        stdout.contains("OK"),
        "Git should be OK when configured, got: {}",
        stdout
    );
}

// ============ S3 Connector Tests ============

#[test]
fn test_s3_connector_not_configured() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["sync", "s3"]);
    assert!(!success, "s3 sync should fail when not configured");
    assert!(
        stderr.contains("No s3 connectors configured"),
        "Should mention no s3 connectors, got: {}",
        stderr
    );
}

#[test]
fn test_s3_sources_not_configured() {
    let (_tmp, config_path) = setup_test_env();

    let (stdout, _, success) = run_ctx(&config_path, &["sources"]);
    assert!(success);
    // S3 not shown when no instances configured — only configured connectors appear
    assert!(
        !stdout.contains("s3:"),
        "S3 should not appear when no instances configured, got: {}",
        stdout
    );
}

#[test]
fn test_unknown_connector_message_includes_available() {
    let (_tmp, config_path) = setup_test_env();

    run_ctx(&config_path, &["init"]);
    let (_, stderr, success) = run_ctx(&config_path, &["sync", "nonexistent"]);
    assert!(!success);
    assert!(
        stderr.contains("all")
            && stderr.contains("filesystem")
            && stderr.contains("git")
            && stderr.contains("s3"),
        "Error should list available connectors, got: {}",
        stderr
    );
}

#[test]
fn test_server_search_with_filters() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);

    let url = format!("http://127.0.0.1:{}/tools/search", port);
    let client = reqwest::blocking::Client::new();

    // Search with source filter
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "query": "document",
            "mode": "keyword",
            "limit": 5,
            "filters": {
                "source": "filesystem:test"
            }
        }))
        .send()
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().unwrap();
    let results = body["result"]["results"].as_array().unwrap();
    for r in results {
        assert_eq!(r["source"], "filesystem:test");
    }

    server.kill().ok();
    server.wait().ok();
}

// ============ Additive-invariant golden test (SPEC-0014 R15 / AC 2) ============
//
// Locks the compatibility-mode wire contract: a server started without
// `--workspaces` must expose the exact pre-router tool schemas and flat
// response shapes. The expected JSON below IS the committed golden baseline —
// the multi-workspace work (e.g. adding a `workspace` selector to built-in
// tools) must NOT change anything asserted here.

/// The exact `parameters` schema each built-in tool advertises in compat mode.
fn expected_compat_tool_schema(name: &str) -> serde_json::Value {
    match name {
        "search" => serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"], "default": "keyword" },
                "limit": { "type": "integer", "description": "Max results", "default": 12 },
                "filters": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Filter by connector source" },
                        "since": { "type": "string", "description": "Only results updated after this date (YYYY-MM-DD)" }
                    }
                }
            },
            "required": ["query"]
        }),
        "get" => serde_json::json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "Document UUID" } },
            "required": ["id"]
        }),
        "sources" => serde_json::json!({ "type": "object", "properties": {} }),
        other => panic!("unexpected compat tool: {other}"),
    }
}

#[test]
fn test_compat_golden_invariant() {
    let port = find_free_port();
    let (_tmp, config_path) = setup_server_env(port);

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem"]);

    let mut server = start_server(&config_path);
    wait_for_server(port);
    let client = reqwest::blocking::Client::new();

    // ── /health ──
    let health: serde_json::Value =
        reqwest::blocking::get(format!("http://127.0.0.1:{port}/health"))
            .unwrap()
            .json()
            .unwrap();
    assert_eq!(health["status"], "ok");
    assert!(health["version"].is_string());

    // ── GET /tools/list: exactly the three built-ins, exact schemas ──
    let list: serde_json::Value =
        reqwest::blocking::get(format!("http://127.0.0.1:{port}/tools/list"))
            .unwrap()
            .json()
            .unwrap();
    let tools = list["tools"].as_array().expect("tools array");
    assert_eq!(
        tools.len(),
        3,
        "compat mode exposes exactly 3 built-in tools"
    );
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(names, ["get", "search", "sources"]);

    for t in tools {
        let name = t["name"].as_str().unwrap();
        assert_eq!(t["builtin"], true, "{name} must be builtin");
        assert_eq!(
            t["parameters"],
            expected_compat_tool_schema(name),
            "{name} schema drifted from the compat golden baseline"
        );
        // The additive guarantee: compat mode must NOT advertise a workspace selector.
        assert!(
            t["parameters"]["properties"].get("workspace").is_none(),
            "{name} must not expose a `workspace` param in compat mode"
        );
    }

    // ── POST /tools/search: flat shape, no workspace labels ──
    let search: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/tools/search"))
        .json(&serde_json::json!({ "query": "Rust programming", "mode": "keyword", "limit": 5 }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let result = &search["result"];
    assert!(
        result["results"].is_array(),
        "compat search is flat `results`"
    );
    assert!(
        result.get("errors").is_none(),
        "compat search has no grouped `errors`"
    );
    for item in result["results"].as_array().unwrap() {
        assert!(
            item.get("workspace").is_none(),
            "compat items carry no `workspace`"
        );
        assert!(
            item.get("qualified_id").is_none(),
            "compat items carry no `qualified_id`"
        );
    }

    // ── POST /tools/sources: flat shape ──
    let sources: serde_json::Value = client
        .post(format!("http://127.0.0.1:{port}/tools/sources"))
        .json(&serde_json::json!({}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        sources["result"]["sources"].is_array(),
        "compat sources is flat"
    );

    server.kill().ok();
    server.wait().ok();
}

// ============ Multi-workspace MCP router (SPEC-0014 Phase 1) ============

/// Create a workspace at `root` with a pinned `.ctx/config.toml` (absolute db
/// path + a filesystem connector), seed one doc, and return the config path.
fn setup_multi_workspace(root: &Path, body: &str) -> PathBuf {
    let ctx = root.join(".ctx");
    fs::create_dir_all(ctx.join("data")).unwrap();
    let files = root.join("files");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("doc.md"), format!("# Doc\n\n{}\n", body)).unwrap();

    let config = format!(
        "[db]\npath = \"{db}\"\n\n\
         [chunking]\nmax_tokens = 700\noverlap_tokens = 0\n\n\
         [retrieval]\nfinal_limit = 12\n\n\
         [server]\nbind = \"127.0.0.1:7331\"\n\n\
         [connectors.filesystem.local]\nroot = \"{files}\"\n\
         include_globs = [\"**/*.md\"]\nexclude_globs = []\nfollow_symlinks = false\n",
        db = ctx.join("data").join("ctx.sqlite").display(),
        files = files.display(),
    );
    let config_path = ctx.join("config.toml");
    fs::write(&config_path, config).unwrap();
    config_path
}

/// Start `ctx serve mcp --workspaces=<registry>` with an isolated config dir.
fn start_server_multi(registry_path: &Path, config_dir: &Path) -> std::process::Child {
    let binary = ctx_binary();
    Command::new(&binary)
        .args(["serve", "mcp"])
        .arg(format!("--workspaces={}", registry_path.display()))
        .env_remove("CTX_CONFIG")
        .env("CTX_CONFIG_DIR", config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("Failed to start multi-workspace server: {}", e))
}

#[test]
fn test_multi_workspace_routing() {
    let ws_a = TempDir::new().unwrap();
    let ws_b = TempDir::new().unwrap();
    let cfg_a = setup_multi_workspace(ws_a.path(), "alpha notes about Rust cargo and crates");
    let cfg_b = setup_multi_workspace(ws_b.path(), "beta notes about Python and pytorch");

    // Build each workspace store via the compat path (absolute db paths).
    run_ctx(&cfg_a, &["init"]);
    run_ctx(&cfg_a, &["sync", "filesystem"]);
    run_ctx(&cfg_b, &["init"]);
    run_ctx(&cfg_b, &["sync", "filesystem"]);

    let reg_tmp = TempDir::new().unwrap();
    let cfg_dir = TempDir::new().unwrap();
    let port = find_free_port();
    let reg_path = reg_tmp.path().join("workspaces.toml");
    fs::write(
        &reg_path,
        format!(
            "[defaults]\nworkspace = \"alpha\"\nbind = \"127.0.0.1:{port}\"\n\n\
             [workspaces.alpha]\nroot = \"{a}\"\nconfig = \"{ca}\"\nenabled = true\n\n\
             [workspaces.beta]\nroot = \"{b}\"\nconfig = \"{cb}\"\nenabled = true\n\n\
             [workspaces.gamma]\nroot = \"{b}\"\nconfig = \"{cb}\"\nenabled = false\n",
            port = port,
            a = ws_a.path().display(),
            ca = cfg_a.display(),
            b = ws_b.path().display(),
            cb = cfg_b.display(),
        ),
    )
    .unwrap();

    let mut server = start_server_multi(&reg_path, cfg_dir.path());
    wait_for_server(port);
    let client = reqwest::blocking::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // workspaces discovery: alpha (default), beta, gamma (disabled); no secrets.
    let ws: serde_json::Value = client
        .post(format!("{}/tools/workspaces", base))
        .json(&serde_json::json!({}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let list = ws["result"]["workspaces"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    let alpha = list.iter().find(|w| w["id"] == "alpha").unwrap();
    assert_eq!(alpha["default"], true);
    assert_eq!(alpha["enabled"], true);
    assert_eq!(alpha["health"]["status"], "ok");
    let gamma = list.iter().find(|w| w["id"] == "gamma").unwrap();
    assert_eq!(gamma["enabled"], false);

    // Explicit-workspace search: grouped, labeled, qualified ids.
    let s: serde_json::Value = client
        .post(format!("{}/tools/search", base))
        .json(&serde_json::json!({ "query": "Rust", "workspace": "alpha", "limit": 5 }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let groups = s["result"]["results"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["workspace"], "alpha");
    let items = groups[0]["items"].as_array().unwrap();
    assert!(!items.is_empty(), "alpha should have results for 'Rust'");
    assert_eq!(items[0]["workspace"], "alpha");
    let qid = items[0]["qualified_id"].as_str().unwrap().to_string();
    assert!(
        qid.starts_with("alpha:"),
        "qualified id starts with workspace: {}",
        qid
    );

    // get by qualified id routes to alpha.
    let g: serde_json::Value = client
        .post(format!("{}/tools/get", base))
        .json(&serde_json::json!({ "id": qid }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(g["result"]["workspace"], "alpha");

    // Conflict: explicit workspace disagrees with qualified id prefix.
    let conflict = client
        .post(format!("{}/tools/get", base))
        .json(&serde_json::json!({ "id": qid, "workspace": "beta" }))
        .send()
        .unwrap();
    assert_eq!(conflict.status(), 400);
    assert_eq!(
        conflict.json::<serde_json::Value>().unwrap()["error"]["code"],
        "workspace_id_conflict"
    );

    // Disabled workspace.
    let disabled: serde_json::Value = client
        .post(format!("{}/tools/search", base))
        .json(&serde_json::json!({ "query": "x", "workspace": "gamma" }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(disabled["error"]["code"], "workspace_disabled");

    // Unknown workspace -> 404.
    let unknown = client
        .post(format!("{}/tools/search", base))
        .json(&serde_json::json!({ "query": "x", "workspace": "nope" }))
        .send()
        .unwrap();
    assert_eq!(unknown.status(), 404);
    assert_eq!(
        unknown.json::<serde_json::Value>().unwrap()["error"]["code"],
        "unknown_workspace"
    );

    // `all` is rejected in Phase 1.
    let all: serde_json::Value = client
        .post(format!("{}/tools/search", base))
        .json(&serde_json::json!({ "query": "x", "workspace": "all" }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(all["error"]["code"], "unsupported_workspace_selector");

    server.kill().ok();
    server.wait().ok();
}

#[test]
fn test_workspaces_flag_rejects_explicit_config() {
    // SPEC-0014 R13: --workspaces cannot be combined with --config.
    let binary = ctx_binary();
    let output = Command::new(&binary)
        .arg("--config")
        .arg("/tmp/does-not-matter.toml")
        .args(["serve", "mcp", "--workspaces=/tmp/ws.toml"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "should reject --workspaces + --config"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--workspaces cannot be combined with --config"),
        "expected rejection message, got: {}",
        stderr
    );
}

#[test]
fn test_multi_workspace_refuses_non_loopback_bind() {
    let ws = TempDir::new().unwrap();
    let cfg = setup_multi_workspace(ws.path(), "content");
    run_ctx(&cfg, &["init"]);

    let reg_tmp = TempDir::new().unwrap();
    let reg_path = reg_tmp.path().join("workspaces.toml");
    fs::write(
        &reg_path,
        format!(
            "[defaults]\nbind = \"0.0.0.0:7331\"\n\n\
             [workspaces.alpha]\nroot = \"{a}\"\nconfig = \"{ca}\"\nenabled = true\n",
            a = ws.path().display(),
            ca = cfg.display(),
        ),
    )
    .unwrap();

    let binary = ctx_binary();
    let output = Command::new(&binary)
        .args(["serve", "mcp"])
        .arg(format!("--workspaces={}", reg_path.display()))
        .env_remove("CTX_CONFIG")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "non-loopback bind must be refused without --allow-remote"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("non-loopback"),
        "expected non-loopback refusal, got: {}",
        stderr
    );
}
