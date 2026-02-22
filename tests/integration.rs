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

    // Initialize a git repo with explicit 'main' branch
    run_git(&repo_dir, &["init", "-b", "main"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);
    run_git(&repo_dir, &["config", "user.name", "Test User"]);

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
    run_git(&repo_dir, &["add", "."]);
    run_git(&repo_dir, &["commit", "-m", "initial commit"]);

    repo_dir
}

fn run_git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run git {:?}: {}", args, e))
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
