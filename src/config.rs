use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub db: DbConfig,
    pub chunking: ChunkingConfig,
    pub retrieval: RetrievalConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
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
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,
    #[serde(default = "default_candidate_k")]
    pub candidate_k_keyword: i64,
    #[serde(default = "default_candidate_k")]
    pub candidate_k_vector: i64,
    #[serde(default = "default_final_limit")]
    pub final_limit: i64,
    #[serde(default = "default_group_by")]
    #[allow(dead_code)]
    pub group_by: String,
    #[serde(default = "default_doc_agg")]
    #[allow(dead_code)]
    pub doc_agg: String,
    #[serde(default = "default_max_chunks_per_doc")]
    #[allow(dead_code)]
    pub max_chunks_per_doc: usize,
}

fn default_hybrid_alpha() -> f64 {
    0.6
}
fn default_candidate_k() -> i64 {
    80
}
fn default_final_limit() -> i64 {
    12
}
fn default_group_by() -> String {
    "document".to_string()
}
fn default_doc_agg() -> String {
    "max".to_string()
}
fn default_max_chunks_per_doc() -> usize {
    3
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddingConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub dims: Option<usize>,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: "disabled".to_string(),
            model: None,
            dims: None,
            batch_size: 64,
            max_retries: 5,
            timeout_secs: 30,
        }
    }
}

fn default_provider() -> String {
    "disabled".to_string()
}
fn default_batch_size() -> usize {
    64
}
fn default_max_retries() -> u32 {
    5
}
fn default_timeout_secs() -> u64 {
    30
}

#[derive(Debug, Deserialize, Clone)]
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

impl EmbeddingConfig {
    pub fn is_enabled(&self) -> bool {
        self.provider != "disabled"
    }
}

pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&content).with_context(|| "Failed to parse config file")?;

    // Validate chunking
    if config.chunking.max_tokens == 0 {
        anyhow::bail!("chunking.max_tokens must be > 0");
    }

    // Validate retrieval
    if config.retrieval.final_limit < 1 {
        anyhow::bail!("retrieval.final_limit must be >= 1");
    }

    if !(0.0..=1.0).contains(&config.retrieval.hybrid_alpha) {
        anyhow::bail!("retrieval.hybrid_alpha must be in [0.0, 1.0]");
    }

    // Validate embedding
    if config.embedding.is_enabled() {
        if config.embedding.dims.is_none() || config.embedding.dims == Some(0) {
            anyhow::bail!(
                "embedding.dims must be > 0 when provider is '{}'",
                config.embedding.provider
            );
        }
        if config.embedding.model.is_none() {
            anyhow::bail!(
                "embedding.model must be specified when provider is '{}'",
                config.embedding.provider
            );
        }
    }

    match config.embedding.provider.as_str() {
        "disabled" | "openai" | "local" => {}
        other => anyhow::bail!(
            "Unknown embedding provider: '{}'. Must be disabled, openai, or local.",
            other
        ),
    }

    Ok(config)
}
