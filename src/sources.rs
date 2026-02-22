use anyhow::Result;
use serde::Serialize;

use crate::config::Config;

/// Source status matching SCHEMAS.md `context.sources` response shape.
#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub configured: bool,
    pub healthy: bool,
    pub notes: Option<String>,
}

/// Core function returning structured source data (used by CLI and server).
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

/// CLI entry point — calls get_sources and prints to stdout.
pub fn list_sources(config: &Config) -> Result<()> {
    let sources = get_sources(config);

    println!("{:<16} {:<12} HEALTHY", "CONNECTOR", "STATUS");
    for s in &sources {
        let status_str = if s.configured { "OK" } else { "NOT CONFIGURED" };
        println!("{:<16} {:<12} {}", s.name, status_str, s.healthy);
    }

    Ok(())
}
