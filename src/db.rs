//! SQLite database connection management.
//!
//! Provides a connection pool to the SQLite database with WAL mode
//! enabled for concurrent read/write performance. The database file
//! and its parent directories are created automatically if they don't exist.
//!
//! # Write-Ahead Logging (WAL)
//!
//! WAL mode is enabled for all connections, which allows concurrent
//! readers and a single writer without blocking. This is important for
//! the MCP server, where search queries and sync operations may overlap.
//!
//! # Connection Pool
//!
//! Uses `sqlx::SqlitePool` with up to 5 concurrent connections.
//! Connections are reused across requests for efficiency.

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;

use crate::config::Config;

/// Create a connection pool to the configured SQLite database.
///
/// - Creates the database file and parent directories if they don't exist.
/// - Enables WAL journal mode for concurrent read/write.
/// - Returns a pool with up to 5 connections.
///
/// # Arguments
///
/// * `config` — Application configuration containing the database path.
///
/// # Errors
///
/// Returns an error if the database cannot be created or connected to.
pub async fn connect(config: &Config) -> Result<SqlitePool> {
    let db_path = &config.db.path;

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", db_path.display()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    Ok(pool)
}
