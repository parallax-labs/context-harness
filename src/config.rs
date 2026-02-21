use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub db: DbConfig,
    pub chunking: ChunkingConfig,
    pub retrieval: RetrievalConfig,
    #[allow(dead_code)]
    pub server: ServerConfig,
    #[serde(default)]
    pub connectors: ConnectorsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DbConfig {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChunkingConfig {
    pub max_tokens: usize,
    #[serde(default = "default_overlap")]
    #[allow(dead_code)]
    pub overlap_tokens: usize,
}

fn default_overlap() -> usize {
    0
}

#[derive(Debug, Deserialize, Clone)]
pub struct RetrievalConfig {
    #[serde(default = "default_final_limit")]
    pub final_limit: i64,
}

fn default_final_limit() -> i64 {
    12
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct ServerConfig {
    pub bind: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ConnectorsConfig {
    pub filesystem: Option<FilesystemConnectorConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FilesystemConnectorConfig {
    pub root: PathBuf,
    #[serde(default = "default_include_globs")]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub follow_symlinks: bool,
}

fn default_include_globs() -> Vec<String> {
    vec!["**/*.md".to_string(), "**/*.txt".to_string()]
}

pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&content).with_context(|| "Failed to parse config file")?;

    // Validate
    if config.chunking.max_tokens == 0 {
        anyhow::bail!("chunking.max_tokens must be > 0");
    }

    if config.retrieval.final_limit < 1 {
        anyhow::bail!("retrieval.final_limit must be >= 1");
    }

    Ok(config)
}
