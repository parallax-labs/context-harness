//! Connector health and status listing.
//!
//! Reports which connectors are configured and healthy. Used by both the
//! `ctx sources` CLI command and the `GET /tools/sources` HTTP endpoint.
//!
//! # Health Checks
//!
//! Each connector performs a lightweight health check:
//!
//! | Connector | Healthy When |
//! |-----------|-------------|
//! | `filesystem` | Configured root directory exists |
//! | `git` | `git --version` succeeds (binary is on PATH) |
//! | `s3` | Always `true` if configured (credentials checked at sync time) |
//! | `slack`, `jira` | Placeholder — always `NOT CONFIGURED` |

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;

/// Health and configuration status of a single connector.
///
/// This struct matches the `context.sources` response shape defined in
/// `docs/SCHEMAS.md`. It is serialized as JSON by the HTTP server.
#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    /// The connector name (e.g., `"filesystem"`, `"git"`, `"s3"`).
    pub name: String,
    /// Whether the connector has a `[connectors.<name>]` section in the config.
    pub configured: bool,
    /// Whether the connector passes its health check.
    pub healthy: bool,
    /// Optional diagnostic notes (e.g., `"root directory does not exist"`, `"repo: https://…"`).
    pub notes: Option<String>,
}

/// Returns the configuration and health status of all known connectors.
///
/// This is the core function used by both the CLI (`ctx sources`) and the
/// HTTP server (`GET /tools/sources`). It checks each connector's config
/// and performs a lightweight health probe.
///
/// All connector types use named instances (e.g. `filesystem:docs`, `git:platform`).
pub fn get_sources(config: &Config) -> Vec<SourceStatus> {
    let mut sources = Vec::new();

    // Filesystem connectors
    for (name, fs_config) in &config.connectors.filesystem {
        if fs_config.root.exists() {
            sources.push(SourceStatus {
                name: format!("filesystem:{}", name),
                configured: true,
                healthy: true,
                notes: Some(format!("root: {}", fs_config.root.display())),
            });
        } else {
            sources.push(SourceStatus {
                name: format!("filesystem:{}", name),
                configured: true,
                healthy: false,
                notes: Some("root directory does not exist".to_string()),
            });
        }
    }

    // Git connectors
    let git_available = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    for (name, git_config) in &config.connectors.git {
        if git_available {
            sources.push(SourceStatus {
                name: format!("git:{}", name),
                configured: true,
                healthy: true,
                notes: Some(format!("repo: {}", git_config.url)),
            });
        } else {
            sources.push(SourceStatus {
                name: format!("git:{}", name),
                configured: true,
                healthy: false,
                notes: Some("git binary not found".to_string()),
            });
        }
    }

    // S3 connectors
    for (name, s3_config) in &config.connectors.s3 {
        sources.push(SourceStatus {
            name: format!("s3:{}", name),
            configured: true,
            healthy: true,
            notes: Some(format!("bucket: {}", s3_config.bucket)),
        });
    }

    // Script connectors
    for (name, script_config) in &config.connectors.script {
        let path_exists = script_config.path.exists();
        sources.push(SourceStatus {
            name: format!("script:{}", name),
            configured: true,
            healthy: path_exists,
            notes: if path_exists {
                Some(format!("path: {}", script_config.path.display()))
            } else {
                Some(format!(
                    "script not found: {}",
                    script_config.path.display()
                ))
            },
        });
    }

    sources
}

/// CLI entry point for `ctx sources`.
///
/// Calls [`get_sources`] and prints a formatted table of connector statuses to stdout.
pub fn list_sources(config: &Config) -> Result<()> {
    let sources = get_sources(config);

    println!("{:<16} {:<12} HEALTHY", "CONNECTOR", "STATUS");
    for s in &sources {
        let status_str = if s.configured { "OK" } else { "NOT CONFIGURED" };
        println!("{:<16} {:<12} {}", s.name, status_str, s.healthy);
    }

    Ok(())
}
