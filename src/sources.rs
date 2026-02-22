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
pub fn get_sources(config: &Config) -> Vec<SourceStatus> {
    let mut sources = Vec::new();

    // Filesystem connector
    let fs_status = match &config.connectors.filesystem {
        Some(fs_config) => {
            if fs_config.root.exists() {
                SourceStatus {
                    name: "filesystem".to_string(),
                    configured: true,
                    healthy: true,
                    notes: None,
                }
            } else {
                SourceStatus {
                    name: "filesystem".to_string(),
                    configured: true,
                    healthy: false,
                    notes: Some("root directory does not exist".to_string()),
                }
            }
        }
        None => SourceStatus {
            name: "filesystem".to_string(),
            configured: false,
            healthy: false,
            notes: None,
        },
    };
    sources.push(fs_status);

    // Git connector
    let git_status = match &config.connectors.git {
        Some(git_config) => {
            // Check if git is available
            let git_available = std::process::Command::new("git")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if git_available {
                SourceStatus {
                    name: "git".to_string(),
                    configured: true,
                    healthy: true,
                    notes: Some(format!("repo: {}", git_config.url)),
                }
            } else {
                SourceStatus {
                    name: "git".to_string(),
                    configured: true,
                    healthy: false,
                    notes: Some("git binary not found".to_string()),
                }
            }
        }
        None => SourceStatus {
            name: "git".to_string(),
            configured: false,
            healthy: false,
            notes: None,
        },
    };
    sources.push(git_status);

    // S3 connector
    let s3_status = match &config.connectors.s3 {
        Some(s3_config) => SourceStatus {
            name: "s3".to_string(),
            configured: true,
            healthy: true,
            notes: Some(format!("bucket: {}", s3_config.bucket)),
        },
        None => SourceStatus {
            name: "s3".to_string(),
            configured: false,
            healthy: false,
            notes: None,
        },
    };
    sources.push(s3_status);

    // Placeholder connectors (future)
    for name in &["slack", "jira"] {
        sources.push(SourceStatus {
            name: name.to_string(),
            configured: false,
            healthy: false,
            notes: None,
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
