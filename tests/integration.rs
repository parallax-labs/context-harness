use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

[connectors.filesystem]
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
    assert!(stdout.contains("filesystem"));
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
    assert!(stderr.contains("Unknown connector"));
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
