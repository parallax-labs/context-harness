//! Filesystem connector.
//!
//! Walks a local directory, applies glob include/exclude patterns, and produces
//! [`SourceItem`]s with filesystem metadata (modification time, file path).
//!
//! # Configuration
//!
//! ```toml
//! [connectors.filesystem.docs]
//! root = "./docs"
//! include_globs = ["**/*.md", "**/*.txt"]
//! exclude_globs = ["**/drafts/**"]
//! follow_symlinks = false
//! ```
//!
//! # Default Excludes
//!
//! The following directories are always excluded regardless of configuration:
//! - `**/.git/**`
//! - `**/target/**`
//! - `**/node_modules/**`
//!
//! # Output
//!
//! Each file becomes a [`SourceItem`] with:
//! - `source`: `"filesystem:<name>"` (e.g. `"filesystem:docs"`)
//! - `source_id`: relative path from root (e.g. `"guides/deploy.md"`)
//! - `source_url`: `file://` URI
//! - `updated_at`: filesystem modification time
//! - `body`: file contents as UTF-8

use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;
use walkdir::WalkDir;

use crate::config::FilesystemConnectorConfig;
use crate::models::SourceItem;
use crate::traits::Connector;

/// Binary file extensions that are read as bytes and extracted (spec §2.2).
const BINARY_EXTENSIONS: &[&str] = &[".pdf", ".docx", ".pptx", ".xlsx"];

/// Extension to MIME type per spec §4.1.
fn binary_content_type(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        ".pdf" => Some("application/pdf"),
        ".docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        ".pptx" => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
        }
        ".xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Connector trait implementation
// ═══════════════════════════════════════════════════════════════════════

/// A filesystem connector instance that implements the [`Connector`] trait.
///
/// Wraps the [`scan_filesystem`] function, allowing filesystem connectors
/// to be used through the unified trait-based dispatch.
///
/// # Example
///
/// ```rust,no_run
/// use context_harness::connector_fs::FilesystemConnector;
/// use context_harness::config::FilesystemConnectorConfig;
/// use context_harness::traits::Connector;
///
/// let config: FilesystemConnectorConfig = toml::from_str(r#"
///     root = "./docs"
///     include_globs = ["**/*.md"]
/// "#).unwrap();
/// let connector = FilesystemConnector::new("docs".into(), config);
/// assert_eq!(connector.source_label(), "filesystem:docs");
/// ```
pub struct FilesystemConnector {
    /// Instance name (e.g. `"docs"`).
    name: String,
    /// Configuration for this filesystem connector instance.
    config: FilesystemConnectorConfig,
}

impl FilesystemConnector {
    /// Create a new filesystem connector instance.
    pub fn new(name: String, config: FilesystemConnectorConfig) -> Self {
        Self { name, config }
    }
}

#[async_trait]
impl Connector for FilesystemConnector {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Walk local directories with glob patterns"
    }

    fn connector_type(&self) -> &str {
        "filesystem"
    }

    async fn scan(&self) -> Result<Vec<SourceItem>> {
        scan_filesystem(&self.name, &self.config)
    }
}

/// Scan a local directory and produce [`SourceItem`]s.
///
/// Walks the configured `root` directory, applies include/exclude globs,
/// reads each matching file, and returns a sorted list of `SourceItem`s.
///
/// # Arguments
///
/// - `name` — the instance name (e.g. `"docs"`). Used as part of the source
///   identifier: `"filesystem:<name>"`.
/// - `fs_config` — the filesystem connector configuration for this instance.
///
/// # Errors
///
/// Returns an error if:
/// - The root directory does not exist
/// - A glob pattern is invalid
/// - A directory entry cannot be read
pub fn scan_filesystem(
    name: &str,
    fs_config: &FilesystemConnectorConfig,
) -> Result<Vec<SourceItem>> {
    let root = &fs_config.root;
    if !root.exists() {
        bail!(
            "Filesystem connector root does not exist: {}",
            root.display()
        );
    }

    let include_set = build_globset(&fs_config.include_globs)?;

    let mut default_excludes = vec![
        "**/.git/**".to_string(),
        "**/target/**".to_string(),
        "**/node_modules/**".to_string(),
    ];
    default_excludes.extend(fs_config.exclude_globs.clone());
    let exclude_set = build_globset(&default_excludes)?;

    let mut items = Vec::new();

    let walker = WalkDir::new(root).follow_links(fs_config.follow_symlinks);
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(path);
        let rel_str = relative.to_string_lossy().to_string();

        // Apply exclude patterns
        if exclude_set.is_match(&rel_str) {
            continue;
        }

        // Apply include patterns
        if !include_set.is_match(&rel_str) {
            continue;
        }

        let source_label = format!("filesystem:{}", name);
        if let Some(item) = file_to_source_item(path, &rel_str, &source_label, fs_config)? {
            items.push(item);
        }
    }

    // Sort for deterministic ordering
    items.sort_by(|a, b| a.source_id.cmp(&b.source_id));

    Ok(items)
}

/// Convert a single file to a [`SourceItem`], or `None` if the file should be skipped (spec §2.2).
///
/// For files with a supported binary extension (.pdf, .docx, .pptx, .xlsx), reads raw bytes
/// and returns an item with `raw_bytes` set and empty `body`. Otherwise reads as UTF-8; on
/// decode failure and binary extension, falls back to raw bytes; else skips (returns `None`).
fn file_to_source_item(
    path: &Path,
    relative_path: &str,
    source: &str,
    _fs_config: &FilesystemConnectorConfig,
) -> Result<Option<SourceItem>> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let modified_secs = modified
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let is_binary_ext = BINARY_EXTENSIONS.contains(&ext.as_str());
    let content_type_from_ext = binary_content_type(&ext);

    if let (true, Some(mime)) = (is_binary_ext, content_type_from_ext) {
        let bytes = std::fs::read(path)?;
        return Ok(Some(SourceItem {
            source: source.to_string(),
            source_id: relative_path.to_string(),
            source_url: Some(format!("file://{}", path.display())),
            title: Some(title),
            author: None,
            created_at: Utc.timestamp_opt(modified_secs, 0).unwrap(),
            updated_at: Utc.timestamp_opt(modified_secs, 0).unwrap(),
            content_type: mime.to_string(),
            body: String::new(),
            metadata_json: "{}".to_string(),
            raw_json: None,
            raw_bytes: Some(bytes),
        }));
    }

    match std::fs::read_to_string(path) {
        Ok(body) => Ok(Some(SourceItem {
            source: source.to_string(),
            source_id: relative_path.to_string(),
            source_url: Some(format!("file://{}", path.display())),
            title: Some(title),
            author: None,
            created_at: Utc.timestamp_opt(modified_secs, 0).unwrap(),
            updated_at: Utc.timestamp_opt(modified_secs, 0).unwrap(),
            content_type: "text/plain".to_string(),
            body,
            metadata_json: "{}".to_string(),
            raw_json: None,
            raw_bytes: None,
        })),
        Err(_) => {
            if let (true, Some(mime)) = (is_binary_ext, content_type_from_ext) {
                let bytes = std::fs::read(path)?;
                Ok(Some(SourceItem {
                    source: source.to_string(),
                    source_id: relative_path.to_string(),
                    source_url: Some(format!("file://{}", path.display())),
                    title: Some(title),
                    author: None,
                    created_at: Utc.timestamp_opt(modified_secs, 0).unwrap(),
                    updated_at: Utc.timestamp_opt(modified_secs, 0).unwrap(),
                    content_type: mime.to_string(),
                    body: String::new(),
                    metadata_json: "{}".to_string(),
                    raw_json: None,
                    raw_bytes: Some(bytes),
                }))
            } else {
                Ok(None)
            }
        }
    }
}

/// Build a [`GlobSet`] from a list of glob pattern strings.
fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}
