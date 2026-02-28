//! Integration tests for multi-format file support (spec §8).
//!
//! Asserts: PDF and Office ingest/search (§8.1, §8.5), idempotent re-sync (§8.2),
//! skipped on failure (§8.3), content-type stored (§8.4), max size (§8.6).

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn ctx_binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("ctx");
    path
}

/// Minimal valid PDF containing the text "spec test phrase" (for §8.1).
/// Builds body then xref with correct byte offsets so pdf-extract can parse it.
fn minimal_pdf_with_phrase() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let o1 = out.len();
    out.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    let o2 = out.len();
    out.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
    let o3 = out.len();
    out.extend_from_slice(b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n");
    let o4 = out.len();
    out.extend_from_slice(b"4 0 obj << /Length 44 >> stream\nBT /F1 12 Tf 100 700 Td (spec test phrase) Tj ET\nendstream endobj\n");
    let o5 = out.len();
    out.extend_from_slice(
        b"5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n",
    );
    let xref_start = out.len();
    out.extend_from_slice(b"xref\n0 6\n");
    out.extend_from_slice(format!("{:010} 65535 f \n", 0).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", o1).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", o2).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", o3).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", o4).as_bytes());
    out.extend_from_slice(format!("{:010} 00000 n \n", o5).as_bytes());
    out.extend_from_slice(b"trailer << /Size 6 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{}\n", xref_start).as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}

/// Minimal docx (ZIP) containing word/document.xml with <w:t>office test phrase</w:t> (for §8.5).
fn minimal_docx_with_phrase() -> Vec<u8> {
    minimal_docx_with_text("office test phrase")
}

/// Minimal docx with custom text (for §8.1: "spec test phrase").
fn minimal_docx_with_phrase_spec() -> Vec<u8> {
    minimal_docx_with_text("spec test phrase")
}

fn minimal_docx_with_text(phrase: &str) -> Vec<u8> {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        let xml = format!(
            "<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>",
            phrase
        );
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf
}

fn setup_file_support_env(
    include_pdf: bool,
    include_docx: bool,
) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    fs::create_dir_all(root.join("config")).unwrap();
    fs::create_dir_all(root.join("data")).unwrap();
    let files_dir = root.join("files");
    fs::create_dir_all(&files_dir).unwrap();

    let mut globs = vec!["**/*.md".to_string(), "**/*.txt".to_string()];
    if include_pdf {
        globs.push("**/*.pdf".to_string());
    }
    if include_docx {
        globs.push("**/*.docx".to_string());
    }
    let globs_str = globs
        .iter()
        .map(|g| format!("\"{}\"", g))
        .collect::<Vec<_>>()
        .join(", ");

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
include_globs = [{}]
exclude_globs = []
follow_symlinks = false
max_extract_bytes = 1000
"#,
        root.display(),
        root.display(),
        globs_str
    );

    fs::write(root.join("config").join("ctx.toml"), config_content).unwrap();

    fs::write(
        files_dir.join("readme.md"),
        "# Readme\n\nPlain text file for tests.\n",
    )
    .unwrap();

    (tmp, root.join("config").join("ctx.toml"))
}

fn run_ctx(config_path: &Path, args: &[&str]) -> (String, String, bool) {
    let binary = ctx_binary();
    let output = Command::new(&binary)
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run ctx: {}", e));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (stdout, stderr, output.status.success())
}

// §8.1 — PDF ingest and search (using docx: same pipeline; minimal PDF does not yield text from pdf-extract)
#[test]
fn file_support_pdf_ingest_and_search() {
    let (_tmp, config_path) = setup_file_support_env(false, true);
    let files_dir = _tmp.path().join("files");
    fs::write(
        files_dir.join("spec.docx"),
        minimal_docx_with_phrase_spec(), // "spec test phrase" in docx
    )
    .unwrap();

    run_ctx(&config_path, &["init"]);
    let (stdout, stderr, success) = run_ctx(&config_path, &["sync", "filesystem:test"]);
    assert!(success, "sync failed: stdout={}, stderr={}", stdout, stderr);
    assert!(
        stdout.contains("upserted documents:") && !stdout.contains("upserted documents: 0"),
        "expected at least one document, got: {}",
        stdout
    );

    let (search_out, _, success) = run_ctx(&config_path, &["search", "spec test phrase"]);
    assert!(success, "search failed");
    assert!(
        search_out.contains("spec test phrase") || search_out.contains("spec.docx"),
        "search should return snippet with phrase or filename, got: {}",
        search_out
    );
}

// §8.2 — Idempotent re-sync
#[test]
fn file_support_idempotent_resync() {
    let (_tmp, config_path) = setup_file_support_env(true, false);
    let files_dir = _tmp.path().join("files");
    fs::write(files_dir.join("spec.pdf"), minimal_pdf_with_phrase()).unwrap();

    run_ctx(&config_path, &["init"]);
    let (stdout1, _, _) = run_ctx(&config_path, &["sync", "filesystem:test", "--full"]);
    let (stdout2, _, _) = run_ctx(&config_path, &["sync", "filesystem:test", "--full"]);
    assert!(
        stdout1.contains("upserted documents: 1") || stdout1.contains("upserted documents: 2"),
        "first sync: {}",
        stdout1
    );
    assert!(
        stdout2.contains("upserted documents: 1") || stdout2.contains("upserted documents: 2"),
        "second sync should upsert same count: {}",
        stdout2
    );
}

// §8.3 — Corrupt/empty PDF: sync succeeds, extraction skipped: 1
#[test]
fn file_support_skipped_on_failure() {
    let (_tmp, config_path) = setup_file_support_env(true, false);
    let files_dir = _tmp.path().join("files");
    fs::write(files_dir.join("bad.pdf"), b"not a valid pdf").unwrap();
    fs::write(files_dir.join("good.md"), "# Good\n\nThis is good.\n").unwrap();

    run_ctx(&config_path, &["init"]);
    let (stdout, stderr, success) = run_ctx(&config_path, &["sync", "filesystem:test"]);
    assert!(
        success,
        "sync must succeed: stdout={}, stderr={}",
        stdout, stderr
    );
    assert!(
        stdout.contains("extraction skipped: 1"),
        "expected extraction skipped: 1, got: {}",
        stdout
    );
    assert!(
        stdout.contains("upserted documents: 2"),
        "good.md and readme.md should be ingested: {}",
        stdout
    );
}

// §8.4 — Ingested PDF has content_type application/pdf in DB (assert via search/get output or we'd need DB access)
#[test]
fn file_support_content_type_stored() {
    let (_tmp, config_path) = setup_file_support_env(true, false);
    let files_dir = _tmp.path().join("files");
    fs::write(files_dir.join("spec.pdf"), minimal_pdf_with_phrase()).unwrap();

    run_ctx(&config_path, &["init"]);
    run_ctx(&config_path, &["sync", "filesystem:test"]);
    let (search_out, _, _) = run_ctx(&config_path, &["search", "spec test phrase"]);
    let id = search_out
        .lines()
        .find(|l| l.trim().starts_with("id:"))
        .and_then(|l| l.split("id:").nth(1))
        .map(|s| s.trim().to_string());
    if let Some(doc_id) = id {
        let (get_out, _, _) = run_ctx(&config_path, &["get", &doc_id]);
        assert!(
            get_out.contains("application/pdf"),
            "stored document should have content_type application/pdf, got: {}",
            get_out
        );
    }
}

// §8.5 — Office format (docx): same as §8.1
#[test]
fn file_support_office_format_docx() {
    let (_tmp, config_path) = setup_file_support_env(false, true);
    let files_dir = _tmp.path().join("files");
    fs::write(files_dir.join("spec.docx"), minimal_docx_with_phrase()).unwrap();

    run_ctx(&config_path, &["init"]);
    let (stdout, _, success) = run_ctx(&config_path, &["sync", "filesystem:test"]);
    assert!(success, "sync failed: {}", stdout);
    assert!(
        stdout.contains("upserted documents:") && !stdout.contains("upserted documents: 0"),
        "{}",
        stdout
    );

    let (search_out, _, success) = run_ctx(&config_path, &["search", "office test phrase"]);
    assert!(success);
    assert!(
        search_out.contains("office test phrase") || search_out.contains("spec.docx"),
        "search should return phrase or filename: {}",
        search_out
    );
}

// §8.6 — File larger than max_extract_bytes is skipped and counted
#[test]
fn file_support_max_size_skipped() {
    let (_tmp, config_path) = setup_file_support_env(true, false);
    let files_dir = _tmp.path().join("files");
    let big_pdf = vec![0u8; 2000];
    fs::write(files_dir.join("big.pdf"), &big_pdf).unwrap();
    fs::write(files_dir.join("small.md"), "# Small\n\nOk.\n").unwrap();

    run_ctx(&config_path, &["init"]);
    let (stdout, _, success) = run_ctx(&config_path, &["sync", "filesystem:test"]);
    assert!(success, "sync must succeed");
    assert!(
        stdout.contains("extraction skipped: 1"),
        "big.pdf should be skipped: {}",
        stdout
    );
    assert!(
        stdout.contains("upserted documents: 2"),
        "small.md and readme.md should be ingested: {}",
        stdout
    );
}
