//! Configuration parsing and validation.
//!
//! Context Harness is configured via a TOML file (default: `config/ctx.toml`).
//! The config defines database paths, chunking parameters, embedding provider
//! settings, retrieval tuning, server bind address, and connector configurations.
//!
//! # Example Configuration
//!
//! ```toml
//! [db]
//! path = "./data/ctx.sqlite"
//!
//! [chunking]
//! max_tokens = 700
//! overlap_tokens = 80
//!
//! [embedding]
//! provider = "openai"           # "disabled" | "openai"
//! model = "text-embedding-3-small"
//! dims = 1536
//!
//! [retrieval]
//! final_limit = 12
//! hybrid_alpha = 0.6            # 0.0 = keyword only, 1.0 = semantic only
//!
//! [server]
//! bind = "127.0.0.1:7331"
//!
//! [connectors.filesystem]
//! root = "./docs"
//! include_globs = ["**/*.md", "**/*.txt"]
//! ```
//!
//! # Connectors
//!
//! Three connector types are supported:
//! - **Filesystem** (`[connectors.filesystem]`) — scan a local directory
//! - **Git** (`[connectors.git]`) — clone/pull a Git repository
//! - **S3** (`[connectors.s3]`) — list and download from an S3 bucket
//!
//! # Validation
//!
//! [`load_config`] performs the following validations:
//! - `chunking.max_tokens > 0`
//! - `retrieval.final_limit >= 1`
//! - `retrieval.hybrid_alpha ∈ [0.0, 1.0]`
//! - When embedding provider ≠ `"disabled"`: `model` and `dims` must be set
//! - Embedding provider must be one of: `"disabled"`, `"openai"`, `"local"`

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level configuration structure.
///
/// Deserialized from the TOML config file. All sections are required
/// except `connectors`, which defaults to an empty set.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// Database connection settings.
    pub db: DbConfig,
    /// Text chunking parameters.
    pub chunking: ChunkingConfig,
    /// Search and retrieval tuning.
    pub retrieval: RetrievalConfig,
    /// Embedding provider settings (defaults to disabled).
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// HTTP server bind address.
    #[allow(dead_code)]
    pub server: ServerConfig,
    /// Connector configurations (all optional).
    #[serde(default)]
    pub connectors: ConnectorsConfig,
}

impl Config {
    /// Create a minimal config suitable for commands that don't need
    /// database or connector settings (e.g., `ctx connector test`).
    pub fn minimal() -> Self {
        Self {
            db: DbConfig {
                path: PathBuf::from("./data/ctx.sqlite"),
            },
            chunking: ChunkingConfig {
                max_tokens: 700,
                overlap_tokens: 0,
            },
            retrieval: RetrievalConfig {
                hybrid_alpha: default_hybrid_alpha(),
                candidate_k_keyword: default_candidate_k(),
                candidate_k_vector: default_candidate_k(),
                final_limit: default_final_limit(),
                group_by: default_group_by(),
                doc_agg: default_doc_agg(),
                max_chunks_per_doc: default_max_chunks_per_doc(),
            },
            embedding: EmbeddingConfig::default(),
            server: ServerConfig {
                bind: "127.0.0.1:7331".to_string(),
            },
            connectors: ConnectorsConfig::default(),
        }
    }
}

/// Database configuration.
///
/// Specifies the path to the SQLite database file. The file and its
/// parent directories are created automatically on first use.
#[derive(Debug, Deserialize, Clone)]
pub struct DbConfig {
    /// Path to the SQLite database file (e.g. `"./data/ctx.sqlite"`).
    pub path: PathBuf,
}

/// Text chunking parameters.
///
/// Controls how document bodies are split into chunks for indexing
/// and embedding. See [`crate::chunk`] for the chunking algorithm.
#[derive(Debug, Deserialize, Clone)]
pub struct ChunkingConfig {
    /// Maximum tokens per chunk. Chunks are split on paragraph boundaries
    /// to stay within this limit. Converted to characters via `max_tokens × 4`.
    pub max_tokens: usize,
    /// Number of overlapping tokens between adjacent chunks (reserved for future use).
    #[serde(default = "default_overlap")]
    #[allow(dead_code)]
    pub overlap_tokens: usize,
}

fn default_overlap() -> usize {
    0
}

/// Search and retrieval tuning parameters.
///
/// These settings control how keyword and semantic search results are
/// merged in hybrid mode, and the overall result limits.
///
/// # Hybrid Scoring
///
/// The `hybrid_alpha` weight determines the blend between keyword (BM25)
/// and semantic (cosine similarity) scores:
///
/// ```text
/// hybrid_score = (1 - α) × keyword_score + α × semantic_score
/// ```
///
/// - `α = 0.0` → pure keyword search
/// - `α = 1.0` → pure semantic search
/// - `α = 0.6` (default) → 60% semantic, 40% keyword
///
/// See `docs/HYBRID_SCORING.md` for the full specification.
#[derive(Debug, Deserialize, Clone)]
pub struct RetrievalConfig {
    /// Weight for semantic vs. keyword scores in hybrid mode.
    /// Range: `[0.0, 1.0]`. Default: `0.6`.
    #[serde(default = "default_hybrid_alpha")]
    pub hybrid_alpha: f64,
    /// Number of keyword candidates to fetch before merging. Default: `80`.
    #[serde(default = "default_candidate_k")]
    pub candidate_k_keyword: i64,
    /// Number of vector candidates to fetch before merging. Default: `80`.
    #[serde(default = "default_candidate_k")]
    pub candidate_k_vector: i64,
    /// Maximum number of results to return after merging and ranking. Default: `12`.
    #[serde(default = "default_final_limit")]
    pub final_limit: i64,
    /// Grouping strategy for results. Default: `"document"`.
    #[serde(default = "default_group_by")]
    #[allow(dead_code)]
    pub group_by: String,
    /// Aggregation method for document-level scores. Default: `"max"`.
    #[serde(default = "default_doc_agg")]
    #[allow(dead_code)]
    pub doc_agg: String,
    /// Maximum chunks per document in results. Default: `3`.
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

/// Embedding provider configuration.
///
/// Controls which embedding provider is used and its parameters.
/// When `provider = "disabled"`, no embeddings are generated and
/// semantic/hybrid search modes will return errors.
///
/// # Providers
///
/// | Provider | Description |
/// |----------|-------------|
/// | `"disabled"` | No embeddings (default) |
/// | `"openai"` | OpenAI API (`text-embedding-3-small`, etc.) |
/// | `"local"` | Reserved for future local model support |
///
/// When using `"openai"`, the `OPENAI_API_KEY` environment variable must be set.
#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddingConfig {
    /// Provider name: `"disabled"`, `"openai"`, or `"local"`. Default: `"disabled"`.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Embedding model name (e.g. `"text-embedding-3-small"`).
    /// Required when provider ≠ `"disabled"`.
    #[serde(default)]
    pub model: Option<String>,
    /// Embedding vector dimensionality (e.g. `1536` for `text-embedding-3-small`).
    /// Required when provider ≠ `"disabled"`.
    #[serde(default)]
    pub dims: Option<usize>,
    /// Number of texts to embed per API call. Default: `64`.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Maximum retry attempts for transient API errors. Default: `5`.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// HTTP timeout per request in seconds. Default: `30`.
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

/// HTTP server configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    /// Socket address to bind to (e.g. `"127.0.0.1:7331"`).
    pub bind: String,
}

/// Container for all connector configurations.
///
/// Each connector is optional — only configured connectors can be used
/// with `ctx sync <connector>`.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ConnectorsConfig {
    /// Filesystem connector: walk a local directory.
    pub filesystem: Option<FilesystemConnectorConfig>,
    /// Git connector: clone and scan a Git repository.
    pub git: Option<GitConnectorConfig>,
    /// S3 connector: list and download from an S3 bucket.
    pub s3: Option<S3ConnectorConfig>,
    /// Named Lua script connectors.
    /// Each key is a connector name, each value contains the script path
    /// and arbitrary config keys passed to the Lua `connector.scan()` function.
    /// See `docs/LUA_CONNECTORS.md` for the full specification.
    #[serde(default)]
    pub script: HashMap<String, ScriptConnectorConfig>,
}

/// Lua script connector configuration.
///
/// Points to a `.lua` file implementing the connector interface. All fields
/// except `path` and `timeout` are passed as a config table to the script's
/// `connector.scan(config)` function.
///
/// Values containing `${VAR_NAME}` are expanded from the process environment.
///
/// # Example
///
/// ```toml
/// [connectors.script.jira]
/// path = "connectors/jira.lua"
/// timeout = 600
/// url = "https://mycompany.atlassian.net"
/// api_token = "${JIRA_API_TOKEN}"
/// project_key = "ENG"
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct ScriptConnectorConfig {
    /// Path to the `.lua` connector script.
    pub path: PathBuf,
    /// Maximum execution time in seconds. Default: `300`.
    #[serde(default = "default_script_timeout")]
    pub timeout: u64,
    /// All other config keys — passed to the Lua `connector.scan()` function.
    #[serde(flatten)]
    pub extra: toml::Table,
}

fn default_script_timeout() -> u64 {
    300
}

/// Filesystem connector configuration.
///
/// Scans a local directory tree, applying glob include/exclude filters.
/// See [`crate::connector_fs`] for the scanning implementation.
///
/// # Example
///
/// ```toml
/// [connectors.filesystem]
/// root = "./docs"
/// include_globs = ["**/*.md", "**/*.txt"]
/// exclude_globs = ["**/drafts/**"]
/// follow_symlinks = false
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct FilesystemConnectorConfig {
    /// Root directory to scan.
    pub root: PathBuf,
    /// Glob patterns for files to include. Default: `["**/*.md", "**/*.txt"]`.
    #[serde(default = "default_include_globs")]
    pub include_globs: Vec<String>,
    /// Glob patterns for files to exclude. Default: `[]`.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Whether to follow symbolic links. Default: `false`.
    #[serde(default)]
    pub follow_symlinks: bool,
}

/// Git connector configuration.
///
/// Clones (or pulls) a Git repository and scans files within a configurable
/// subdirectory. Extracts per-file metadata from `git log`.
/// See [`crate::connector_git`] for the full implementation.
///
/// # Example
///
/// ```toml
/// [connectors.git]
/// url = "https://github.com/acme/platform.git"
/// branch = "main"
/// root = "docs/"
/// include_globs = ["**/*.md"]
/// shallow = true
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct GitConnectorConfig {
    /// Git repository URL (`https://`, `git@`, or local path).
    pub url: String,
    /// Branch to clone/pull. Default: `"main"`.
    #[serde(default = "default_git_branch")]
    pub branch: String,
    /// Subdirectory within the repo to scan. Default: `"."` (entire repo).
    #[serde(default = "default_git_root")]
    pub root: String,
    /// Glob patterns for files to include. Default: `["**/*.md", "**/*.txt"]`.
    #[serde(default = "default_include_globs")]
    pub include_globs: Vec<String>,
    /// Glob patterns for files to exclude. Default: `[]`.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Use shallow clone (`--depth 1`) to save disk space. Default: `true`.
    #[serde(default = "default_true")]
    pub shallow: bool,
    /// Directory to cache cloned repos. Default: `&lt;db-dir&gt;/.git-cache/&lt;url-hash&gt;/`.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

/// Amazon S3 connector configuration.
///
/// Lists and downloads objects from an S3 bucket using the REST API with
/// AWS Signature V4. Supports custom endpoints for S3-compatible services.
/// See [`crate::connector_s3`] for the full implementation.
///
/// # Environment Variables
///
/// - `AWS_ACCESS_KEY_ID` — required
/// - `AWS_SECRET_ACCESS_KEY` — required
/// - `AWS_SESSION_TOKEN` — optional (for temporary credentials)
///
/// # Example
///
/// ```toml
/// [connectors.s3]
/// bucket = "acme-docs"
/// prefix = "engineering/runbooks/"
/// region = "us-east-1"
/// include_globs = ["**/*.md"]
/// # endpoint_url = "http://localhost:9000"   # for MinIO
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct S3ConnectorConfig {
    /// S3 bucket name.
    pub bucket: String,
    /// Key prefix to filter objects. Default: `""` (entire bucket).
    #[serde(default)]
    pub prefix: String,
    /// AWS region. Default: `"us-east-1"`.
    #[serde(default = "default_s3_region")]
    pub region: String,
    /// Glob patterns for object keys to include. Default: `["**/*.md", "**/*.txt"]`.
    #[serde(default = "default_include_globs")]
    pub include_globs: Vec<String>,
    /// Glob patterns for object keys to exclude. Default: `[]`.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Custom endpoint URL for S3-compatible services (MinIO, LocalStack).
    #[serde(default)]
    pub endpoint_url: Option<String>,
}

fn default_git_branch() -> String {
    "main".to_string()
}

fn default_git_root() -> String {
    ".".to_string()
}

fn default_true() -> bool {
    true
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}

fn default_include_globs() -> Vec<String> {
    vec!["**/*.md".to_string(), "**/*.txt".to_string()]
}

impl EmbeddingConfig {
    /// Returns `true` if an embedding provider is configured (not `"disabled"`).
    pub fn is_enabled(&self) -> bool {
        self.provider != "disabled"
    }
}

/// Load and validate a configuration file from disk.
///
/// # Arguments
///
/// * `path` — Path to a TOML configuration file.
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be read or parsed
/// - `chunking.max_tokens` is zero
/// - `retrieval.final_limit` is less than 1
/// - `retrieval.hybrid_alpha` is outside `[0.0, 1.0]`
/// - Embedding provider is enabled but `model` or `dims` is missing/zero
/// - Unknown embedding provider name
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
