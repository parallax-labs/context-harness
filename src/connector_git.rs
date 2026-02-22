//! Git repository connector.
//!
//! Clones or updates a Git repository and walks files within a configurable
//! subdirectory. Extracts rich metadata from `git log`: per-file commit
//! timestamps, authors, and the HEAD commit SHA. Automatically generates
//! web-browsable URLs for GitHub and GitLab repositories.
//!
//! # Configuration
//!
//! ```toml
//! [connectors.git.platform]
//! url = "https://github.com/acme/platform.git"
//! branch = "main"
//! root = "docs/"
//! include_globs = ["**/*.md"]
//! shallow = true
//! ```
//!
//! # Cache Directory
//!
//! Cloned repos are cached locally (default: alongside the SQLite DB in
//! `data/.git-cache/<url-hash>/`). Subsequent syncs do `git fetch && reset`.
//!
//! # Metadata Extraction
//!
//! For each file, the connector extracts:
//! - **`updated_at`** — last commit timestamp from `git log -1 --format=%ct`
//! - **`author`** — last committer name from `git log -1 --format=%an`
//! - **`source_url`** — web URL (GitHub/GitLab blob link) for the file
//! - **`metadata_json`** — JSON with `git_sha` and `repo_url`
//!
//! # Web URL Generation
//!
//! The connector auto-detects GitHub and GitLab URLs and generates
//! browsable blob links:
//!
//! | Input URL | Generated URL |
//! |-----------|--------------|
//! | `git@github.com:org/repo.git` | `https://github.com/org/repo/blob/<sha>/<path>` |
//! | `https://github.com/org/repo.git` | `https://github.com/org/repo/blob/<sha>/<path>` |
//! | `git@gitlab.com:org/repo.git` | `https://gitlab.com/org/repo/-/blob/<sha>/<path>` |
//! | Other | `git://<url>/<path>` |

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

use crate::config::GitConnectorConfig;
use crate::models::SourceItem;
use crate::traits::Connector;

// ═══════════════════════════════════════════════════════════════════════
// Connector trait implementation
// ═══════════════════════════════════════════════════════════════════════

/// A Git connector instance that implements the [`Connector`] trait.
///
/// Wraps the [`scan_git`] function, allowing Git connectors to be used
/// through the unified trait-based dispatch.
pub struct GitConnector {
    /// Instance name (e.g. `"platform"`).
    name: String,
    /// Configuration for this Git connector instance.
    config: GitConnectorConfig,
    /// Path to the SQLite database, used to derive the default cache directory.
    db_path: PathBuf,
}

impl GitConnector {
    /// Create a new Git connector instance.
    pub fn new(name: String, config: GitConnectorConfig, db_path: PathBuf) -> Self {
        Self {
            name,
            config,
            db_path,
        }
    }
}

#[async_trait]
impl Connector for GitConnector {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Clone/pull Git repos and walk files"
    }

    fn connector_type(&self) -> &str {
        "git"
    }

    async fn scan(&self) -> Result<Vec<SourceItem>> {
        scan_git(&self.name, &self.config, &self.db_path)
    }
}

/// Scan a Git repository and produce [`SourceItem`]s.
///
/// # Workflow
///
/// 1. Determine a local cache directory for the clone.
/// 2. Clone (shallow if configured) or pull to update.
/// 3. Walk files under the configured `root` subdirectory.
/// 4. Apply include/exclude globs.
/// 5. Extract per-file metadata from `git log`.
/// 6. Generate web-browsable URLs.
///
/// # Arguments
///
/// - `name` — the instance name (e.g. `"platform"`). Used as part of the
///   source identifier: `"git:<name>"`.
/// - `git_config` — the Git connector configuration for this instance.
/// - `db_path` — path to the SQLite database, used to derive the default cache directory.
///
/// # Errors
///
/// Returns an error if:
/// - `git` binary is not available
/// - Clone or pull fails
/// - The configured `root` subdirectory does not exist in the repo
pub fn scan_git(
    name: &str,
    git_config: &GitConnectorConfig,
    db_path: &Path,
) -> Result<Vec<SourceItem>> {
    // Determine cache directory
    let cache_dir = match &git_config.cache_dir {
        Some(dir) => dir.clone(),
        None => {
            // Default: sibling to the DB file
            let db_parent = db_path.parent().unwrap_or_else(|| Path::new("."));
            let url_hash = short_hash(&git_config.url);
            db_parent.join(".git-cache").join(url_hash)
        }
    };

    // Clone or pull
    if cache_dir.join(".git").exists() {
        git_pull(&cache_dir, &git_config.branch)?;
    } else {
        git_clone(
            &git_config.url,
            &git_config.branch,
            git_config.shallow,
            &cache_dir,
        )?;
    }

    // Resolve the scan root within the cloned repo
    let scan_root = if git_config.root == "." {
        cache_dir.clone()
    } else {
        cache_dir.join(&git_config.root)
    };

    if !scan_root.exists() {
        bail!(
            "Git connector root '{}' does not exist in repo {}",
            git_config.root,
            git_config.url
        );
    }

    // Get the HEAD commit SHA for metadata
    let head_sha = git_head_sha(&cache_dir).unwrap_or_else(|_| "unknown".to_string());

    // Build glob sets
    let include_set = build_globset(&git_config.include_globs)?;

    let mut default_excludes = vec![
        "**/.git/**".to_string(),
        "**/target/**".to_string(),
        "**/node_modules/**".to_string(),
    ];
    default_excludes.extend(git_config.exclude_globs.clone());
    let exclude_set = build_globset(&default_excludes)?;

    let mut items = Vec::new();

    for entry in WalkDir::new(&scan_root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let relative = path.strip_prefix(&scan_root).unwrap_or(path);
        let rel_str = relative.to_string_lossy().to_string();

        if exclude_set.is_match(&rel_str) {
            continue;
        }
        if !include_set.is_match(&rel_str) {
            continue;
        }

        let source_label = format!("git:{}", name);
        let item = file_to_source_item(
            path,
            &rel_str,
            &cache_dir,
            &git_config.url,
            &head_sha,
            &source_label,
        )?;
        items.push(item);
    }

    items.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    Ok(items)
}

/// Clone a Git repository into the cache directory.
///
/// Supports shallow clones (`--depth 1`) and single-branch checkout.
fn git_clone(url: &str, branch: &str, shallow: bool, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create cache directory: {}", dest.display()))?;

    let mut cmd = Command::new("git");
    cmd.args(["clone", "--branch", branch, "--single-branch"]);
    if shallow {
        cmd.args(["--depth", "1"]);
    }
    cmd.arg(url);
    cmd.arg(dest);

    let output = cmd
        .output()
        .with_context(|| "Failed to execute 'git clone'. Is git installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git clone failed: {}", stderr.trim());
    }

    Ok(())
}

/// Update an existing cached repository via fetch + hard reset.
fn git_pull(repo_dir: &Path, branch: &str) -> Result<()> {
    // Fetch the latest changes
    let output = Command::new("git")
        .args(["fetch", "origin", branch])
        .current_dir(repo_dir)
        .output()
        .with_context(|| "Failed to execute 'git fetch'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git fetch failed: {}", stderr.trim());
    }

    // Reset to the fetched branch
    let remote_ref = format!("origin/{}", branch);
    let output = Command::new("git")
        .args(["reset", "--hard", &remote_ref])
        .current_dir(repo_dir)
        .output()
        .with_context(|| "Failed to execute 'git reset'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git reset failed: {}", stderr.trim());
    }

    Ok(())
}

/// Get the HEAD commit SHA of a repository.
fn git_head_sha(repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .with_context(|| "Failed to get HEAD SHA")?;

    if !output.status.success() {
        bail!("git rev-parse HEAD failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the last commit timestamp (Unix epoch) for a specific file.
///
/// Returns `None` if the file has no Git history or `git log` fails.
fn git_file_last_commit_time(repo_dir: &Path, file_path: &Path) -> Option<i64> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct", "--"])
        .arg(file_path)
        .current_dir(repo_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let ts_str = String::from_utf8_lossy(&output.stdout);
    ts_str.trim().parse::<i64>().ok()
}

/// Get the last commit author name for a specific file.
///
/// Returns `None` if the file has no Git history or `git log` fails.
fn git_file_last_author(repo_dir: &Path, file_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%an", "--"])
        .arg(file_path)
        .current_dir(repo_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let author = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if author.is_empty() {
        None
    } else {
        Some(author)
    }
}

/// Convert a file in the cloned repo to a [`SourceItem`].
///
/// Extracts Git metadata (commit timestamp, author) and generates
/// a web-browsable URL for GitHub/GitLab repositories.
fn file_to_source_item(
    path: &Path,
    relative_path: &str,
    repo_dir: &Path,
    repo_url: &str,
    head_sha: &str,
    source: &str,
) -> Result<SourceItem> {
    let body = std::fs::read_to_string(path).unwrap_or_default();

    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Try to get the git commit timestamp for this file; fall back to filesystem mtime
    let commit_ts = git_file_last_commit_time(repo_dir, path);
    let updated_secs = commit_ts.unwrap_or_else(|| {
        let metadata = std::fs::metadata(path).ok();
        metadata
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let author = git_file_last_author(repo_dir, path);

    // Build a web URL if this looks like a GitHub/GitLab repo
    let source_url = build_web_url(repo_url, head_sha, relative_path);

    let metadata = serde_json::json!({
        "git_sha": head_sha,
        "repo_url": repo_url,
    });

    Ok(SourceItem {
        source: source.to_string(),
        source_id: relative_path.to_string(),
        source_url: Some(source_url),
        title: Some(title),
        author,
        created_at: Utc.timestamp_opt(updated_secs, 0).unwrap(),
        updated_at: Utc.timestamp_opt(updated_secs, 0).unwrap(),
        content_type: "text/plain".to_string(),
        body,
        metadata_json: metadata.to_string(),
        raw_json: None,
    })
}

/// Attempt to build a web-browsable URL from the git remote URL.
///
/// Supports GitHub (`git@github.com:` and `https://github.com/`) and
/// GitLab (`git@gitlab.com:`) URL formats. Falls back to `git://` URI.
fn build_web_url(repo_url: &str, sha: &str, relative_path: &str) -> String {
    // Convert git@github.com:org/repo.git → https://github.com/org/repo/blob/<sha>/<path>
    if let Some(rest) = repo_url.strip_prefix("git@github.com:") {
        let repo = rest.trim_end_matches(".git");
        return format!("https://github.com/{}/blob/{}/{}", repo, sha, relative_path);
    }

    // Convert https://github.com/org/repo.git → https://github.com/org/repo/blob/<sha>/<path>
    if repo_url.contains("github.com") {
        let base = repo_url.trim_end_matches(".git");
        return format!("{}/blob/{}/{}", base, sha, relative_path);
    }

    // Convert git@gitlab.com:org/repo.git → https://gitlab.com/org/repo/-/blob/<sha>/<path>
    if let Some(rest) = repo_url.strip_prefix("git@gitlab.com:") {
        let repo = rest.trim_end_matches(".git");
        return format!(
            "https://gitlab.com/{}/-/blob/{}/{}",
            repo, sha, relative_path
        );
    }

    // Fallback: just reference the relative path
    format!("git://{}/{}", repo_url, relative_path)
}

/// Generate a short (12-char) SHA-256 hash of input, used for cache directory naming.
fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())[..12].to_string()
}

/// Build a [`GlobSet`] from a list of glob pattern strings.
fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}
