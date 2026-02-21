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

    // Placeholder connectors
    for name in &["github", "slack", "jira"] {
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
