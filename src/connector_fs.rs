use anyhow::{bail, Result};
use chrono::{TimeZone, Utc};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;
use walkdir::WalkDir;

use crate::config::Config;
use crate::models::SourceItem;

pub fn scan_filesystem(config: &Config) -> Result<Vec<SourceItem>> {
    let fs_config = config
        .connectors
        .filesystem
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Filesystem connector not configured"))?;

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

        let item = file_to_source_item(path, &rel_str)?;
        items.push(item);
    }

    // Sort for deterministic ordering
    items.sort_by(|a, b| a.source_id.cmp(&b.source_id));

    Ok(items)
}

fn file_to_source_item(path: &Path, relative_path: &str) -> Result<SourceItem> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let modified_secs = modified
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let body = std::fs::read_to_string(path).unwrap_or_default();

    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(SourceItem {
        source: "filesystem".to_string(),
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
    })
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}
