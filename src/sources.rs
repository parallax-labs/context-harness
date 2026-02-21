use anyhow::Result;

use crate::config::Config;

pub fn list_sources(config: &Config) -> Result<()> {
    // Filesystem connector
    let fs_status = match &config.connectors.filesystem {
        Some(fs_config) => {
            if fs_config.root.exists() {
                ("OK", true)
            } else {
                ("NOT CONFIGURED (root does not exist)", false)
            }
        }
        None => ("NOT CONFIGURED", false),
    };

    println!("{:<16} {:<12} HEALTHY", "CONNECTOR", "STATUS");
    println!("{:<16} {:<12} {}", "filesystem", fs_status.0, fs_status.1);

    // Placeholder for future connectors
    let not_configured = "NOT CONFIGURED";
    let unhealthy = false;
    println!("{:<16} {:<12} {}", "github", not_configured, unhealthy);
    println!("{:<16} {:<12} {}", "slack", not_configured, unhealthy);
    println!("{:<16} {:<12} {}", "jira", not_configured, unhealthy);

    Ok(())
}
