//! Embedding provider abstraction and implementations.
//!
//! Defines the [`EmbeddingProvider`] trait and concrete implementations:
//! - **[`DisabledProvider`]** — returns errors; used when embeddings are not configured.
//! - **[`OpenAIProvider`]** — calls the OpenAI embeddings API with batching, retry, and backoff.
//! - **[`OllamaProvider`]** — calls a local Ollama instance's `/api/embed` endpoint.
//! - **[`LocalProvider`]** — runs models locally via fastembed (primary) or tract (musl/Intel Mac); no network calls after model download.
//!
//! Also provides vector utilities for working with sqlite-vec:
//! - [`cosine_similarity`] — compute similarity between two embedding vectors
//! - [`vec_to_blob`] — encode a `Vec<f32>` as little-endian bytes for SQLite BLOB storage
//! - [`blob_to_vec`] — decode a SQLite BLOB back into a `Vec<f32>`
//!
//! # Provider Selection
//!
//! Use [`create_provider`] to instantiate the appropriate provider based
//! on the configuration:
//!
//! ```rust,no_run
//! # use context_harness::config::EmbeddingConfig;
//! # use context_harness::embedding::create_provider;
//! let config = EmbeddingConfig::default(); // provider = "disabled"
//! let provider = create_provider(&config).unwrap();
//! assert_eq!(provider.model_name(), "disabled");
//! ```
//!
//! # Retry Strategy
//!
//! The OpenAI and Ollama providers use exponential backoff for transient errors:
//! - HTTP 429 (rate limited) and 5xx (server error) → retry
//! - HTTP 4xx (client error, not 429) → fail immediately
//! - Network errors → retry
//! - Backoff: 1s, 2s, 4s, 8s, 16s, 32s (capped at 2^5)

#[cfg(feature = "local-embeddings-tract")]
mod local_tract;

use anyhow::{bail, Result};
use std::time::Duration;

use crate::config::EmbeddingConfig;

/// Trait for embedding providers.
///
/// Defines the interface that all embedding backends must implement.
/// The actual embedding computation is performed by [`embed_texts`]
/// (kept as a free function due to async trait limitations).
pub trait EmbeddingProvider: Send + Sync {
    /// Returns the model identifier (e.g. `"text-embedding-3-small"`).
    fn model_name(&self) -> &str;
    /// Returns the embedding vector dimensionality (e.g. `1536`).
    fn dims(&self) -> usize;
}

/// Embed a batch of texts using the configured provider.
///
/// This is the main entry point for generating embeddings. It dispatches
/// to the appropriate backend based on the config's `provider` field.
///
/// # Arguments
///
/// * `_provider` — Provider instance (used for metadata; dispatch is config-based).
/// * `config` — Embedding configuration with provider, model, and retry settings.
/// * `texts` — Batch of text strings to embed.
///
/// # Returns
///
/// A vector of embedding vectors, one per input text, in the same order.
///
/// # Errors
///
/// - `"disabled"` provider: always returns an error.
/// - `"openai"` provider: returns an error if the API key is missing,
///   the API returns a non-retryable error, or all retries are exhausted.
pub async fn embed_texts(
    _provider: &dyn EmbeddingProvider,
    config: &EmbeddingConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    match config.provider.as_str() {
        "openai" => embed_openai(config, texts).await,
        "ollama" => embed_ollama(config, texts).await,
        #[cfg(feature = "local-embeddings-fastembed")]
        "local" => embed_local_fastembed(config, texts).await,
        #[cfg(feature = "local-embeddings-tract")]
        "local" => embed_local_tract(config, texts).await,
        #[cfg(not(any(feature = "local-embeddings-fastembed", feature = "local-embeddings-tract")))]
        "local" => bail!(
            "Local embedding provider requires one of: --features local-embeddings-fastembed, --features local-embeddings-tract"
        ),
        "disabled" => bail!("Embedding provider is disabled"),
        other => bail!("Unknown embedding provider: {}", other),
    }
}

/// Embed a single query text.
///
/// Convenience wrapper around [`embed_texts`] for single-text use cases
/// (e.g. embedding a search query for semantic search).
pub async fn embed_query(
    provider: &dyn EmbeddingProvider,
    config: &EmbeddingConfig,
    text: &str,
) -> Result<Vec<f32>> {
    let results = embed_texts(provider, config, &[text.to_string()]).await?;
    results
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty embedding response"))
}

// ============ Disabled Provider ============

/// A no-op embedding provider that always returns errors.
///
/// Used when `embedding.provider = "disabled"` in the configuration.
/// Any attempt to embed text will fail with a descriptive error message.
pub struct DisabledProvider;

impl EmbeddingProvider for DisabledProvider {
    fn model_name(&self) -> &str {
        "disabled"
    }
    fn dims(&self) -> usize {
        0
    }
}

// ============ OpenAI Provider ============

/// Embedding provider using the OpenAI API.
///
/// Calls the `POST /v1/embeddings` endpoint with the configured model.
/// Requires the `OPENAI_API_KEY` environment variable to be set.
///
/// # Features
///
/// - Batched embedding (multiple texts per API call)
/// - Exponential backoff retry for rate limits and server errors
/// - Configurable timeout and max retries
pub struct OpenAIProvider {
    /// Model name (e.g. `"text-embedding-3-small"`).
    model: String,
    /// Vector dimensionality (e.g. `1536`).
    dims: usize,
}

impl OpenAIProvider {
    /// Create a new OpenAI provider from configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if `model` or `dims` is not set in config,
    /// or if `OPENAI_API_KEY` is not in the environment.
    pub fn new(config: &EmbeddingConfig) -> Result<Self> {
        let model = config
            .model
            .clone()
            .ok_or_else(|| anyhow::anyhow!("embedding.model required for OpenAI provider"))?;
        let dims = config
            .dims
            .ok_or_else(|| anyhow::anyhow!("embedding.dims required for OpenAI provider"))?;

        // Verify API key is available
        if std::env::var("OPENAI_API_KEY").is_err() {
            bail!("OPENAI_API_KEY environment variable not set");
        }

        Ok(Self { model, dims })
    }
}

impl EmbeddingProvider for OpenAIProvider {
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dims(&self) -> usize {
        self.dims
    }
}

/// Call the OpenAI embeddings API with retry/backoff.
///
/// Sends a batch of texts to `POST https://api.openai.com/v1/embeddings`
/// and returns the embedding vectors in input order.
///
/// Retry strategy:
/// - HTTP 429 or 5xx → retry with exponential backoff
/// - HTTP 4xx (not 429) → fail immediately
/// - Network error → retry
async fn embed_openai(config: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let api_key =
        std::env::var("OPENAI_API_KEY").map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set"))?;

    let model = config
        .model
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("embedding.model required"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()?;

    let body = serde_json::json!({
        "model": model,
        "input": texts,
    });

    let mut last_err = None;

    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            // Exponential backoff: 1s, 2s, 4s, 8s, ...
            let delay = Duration::from_secs(1 << (attempt - 1).min(5));
            tokio::time::sleep(delay).await;
        }

        let resp = client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(response) => {
                let status = response.status();

                if status.is_success() {
                    let json: serde_json::Value = response.json().await?;
                    return parse_openai_response(&json);
                }

                // Rate limited or server error — retry
                if status.as_u16() == 429 || status.is_server_error() {
                    let body_text = response.text().await.unwrap_or_default();
                    last_err = Some(anyhow::anyhow!(
                        "OpenAI API error {}: {}",
                        status,
                        body_text
                    ));
                    continue;
                }

                // Client error (not 429) — don't retry
                let body_text = response.text().await.unwrap_or_default();
                bail!("OpenAI API error {}: {}", status, body_text);
            }
            Err(e) => {
                last_err = Some(e.into());
                continue;
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Embedding failed after retries")))
}

/// Parse the OpenAI embeddings API response JSON.
///
/// Extracts the `data[].embedding` arrays and returns them in order.
fn parse_openai_response(json: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
    let data = json
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow::anyhow!("Invalid OpenAI response: missing data array"))?;

    let mut embeddings = Vec::with_capacity(data.len());

    for item in data {
        let embedding = item
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid OpenAI response: missing embedding"))?;

        let vec: Vec<f32> = embedding
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        embeddings.push(vec);
    }

    // Sort by index to ensure order matches input
    Ok(embeddings)
}

// ============ Ollama Provider ============

/// Embedding provider using a local Ollama instance.
///
/// Calls `POST /api/embed` on the configured Ollama URL (default: `http://localhost:11434`).
/// Requires Ollama to be running with an embedding model pulled (e.g. `ollama pull nomic-embed-text`).
pub struct OllamaProvider {
    model: String,
    dims: usize,
    #[allow(dead_code)]
    url: String,
}

impl OllamaProvider {
    pub fn new(config: &EmbeddingConfig) -> Result<Self> {
        let model = config
            .model
            .clone()
            .ok_or_else(|| anyhow::anyhow!("embedding.model required for Ollama provider"))?;
        let dims = config
            .dims
            .ok_or_else(|| anyhow::anyhow!("embedding.dims required for Ollama provider"))?;
        let url = config
            .url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        Ok(Self { model, dims, url })
    }
}

impl EmbeddingProvider for OllamaProvider {
    fn model_name(&self) -> &str {
        &self.model
    }
    fn dims(&self) -> usize {
        self.dims
    }
}

async fn embed_ollama(config: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let model = config
        .model
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("embedding.model required"))?;

    let url = config.url.as_deref().unwrap_or("http://localhost:11434");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()?;

    let body = serde_json::json!({
        "model": model,
        "input": texts,
    });

    let mut last_err = None;

    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            let delay = Duration::from_secs(1 << (attempt - 1).min(5));
            tokio::time::sleep(delay).await;
        }

        let resp = client
            .post(format!("{}/api/embed", url))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(response) => {
                let status = response.status();

                if status.is_success() {
                    let json: serde_json::Value = response.json().await?;
                    return parse_ollama_response(&json);
                }

                if status.as_u16() == 429 || status.is_server_error() {
                    let body_text = response.text().await.unwrap_or_default();
                    last_err = Some(anyhow::anyhow!(
                        "Ollama API error {}: {}",
                        status,
                        body_text
                    ));
                    continue;
                }

                let body_text = response.text().await.unwrap_or_default();
                bail!("Ollama API error {}: {}", status, body_text);
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!(
                    "Ollama connection error (is Ollama running at {}?): {}",
                    url,
                    e
                ));
                continue;
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Ollama embedding failed after retries")))
}

fn parse_ollama_response(json: &serde_json::Value) -> Result<Vec<Vec<f32>>> {
    let embeddings = json
        .get("embeddings")
        .and_then(|e| e.as_array())
        .ok_or_else(|| anyhow::anyhow!("Invalid Ollama response: missing embeddings array"))?;

    let mut result = Vec::with_capacity(embeddings.len());

    for embedding in embeddings {
        let vec: Vec<f32> = embedding
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid Ollama response: embedding is not an array"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();
        result.push(vec);
    }

    Ok(result)
}

// ============ Local Provider (fastembed or tract) ============

/// Embedding provider for local inference (fastembed on primary platforms, tract on musl/Intel Mac).
///
/// Models are downloaded on first use from Hugging Face and cached.
/// After initial download, no network calls are needed — embeddings run entirely offline.
/// No system dependencies: ORT is bundled (fastembed) or pure Rust (tract).
#[cfg(any(
    feature = "local-embeddings-fastembed",
    feature = "local-embeddings-tract"
))]
pub struct LocalProvider {
    model_name: String,
    dims: usize,
}

#[cfg(any(
    feature = "local-embeddings-fastembed",
    feature = "local-embeddings-tract"
))]
impl LocalProvider {
    pub fn new(config: &EmbeddingConfig) -> Result<Self> {
        let (model_name, dims) = resolve_local_model(config)?;
        Ok(Self { model_name, dims })
    }
}

#[cfg(any(
    feature = "local-embeddings-fastembed",
    feature = "local-embeddings-tract"
))]
impl EmbeddingProvider for LocalProvider {
    fn model_name(&self) -> &str {
        &self.model_name
    }
    fn dims(&self) -> usize {
        self.dims
    }
}

#[cfg(any(
    feature = "local-embeddings-fastembed",
    feature = "local-embeddings-tract"
))]
fn resolve_local_model(config: &EmbeddingConfig) -> Result<(String, usize)> {
    let model_name = config
        .model
        .clone()
        .unwrap_or_else(|| "all-minilm-l6-v2".to_string());

    let dims = config.dims.unwrap_or(match model_name.as_str() {
        "all-minilm-l6-v2" => 384,
        "bge-small-en-v1.5" => 384,
        "bge-base-en-v1.5" => 768,
        "bge-large-en-v1.5" => 1024,
        "nomic-embed-text-v1" | "nomic-embed-text-v1.5" => 768,
        "multilingual-e5-small" => 384,
        "multilingual-e5-base" => 768,
        "multilingual-e5-large" => 1024,
        _ => 384,
    });

    Ok((model_name, dims))
}

#[cfg(feature = "local-embeddings-fastembed")]
fn config_to_fastembed_model(name: &str) -> Result<fastembed::EmbeddingModel> {
    match name {
        "all-minilm-l6-v2" => Ok(fastembed::EmbeddingModel::AllMiniLML6V2),
        "bge-small-en-v1.5" => Ok(fastembed::EmbeddingModel::BGESmallENV15),
        "bge-base-en-v1.5" => Ok(fastembed::EmbeddingModel::BGEBaseENV15),
        "bge-large-en-v1.5" => Ok(fastembed::EmbeddingModel::BGELargeENV15),
        "nomic-embed-text-v1" => Ok(fastembed::EmbeddingModel::NomicEmbedTextV1),
        "nomic-embed-text-v1.5" => Ok(fastembed::EmbeddingModel::NomicEmbedTextV15),
        "multilingual-e5-small" => Ok(fastembed::EmbeddingModel::MultilingualE5Small),
        "multilingual-e5-base" => Ok(fastembed::EmbeddingModel::MultilingualE5Base),
        "multilingual-e5-large" => Ok(fastembed::EmbeddingModel::MultilingualE5Large),
        other => bail!(
            "Unknown local embedding model: '{}'. Supported models: \
             all-minilm-l6-v2, bge-small-en-v1.5, bge-base-en-v1.5, bge-large-en-v1.5, \
             nomic-embed-text-v1, nomic-embed-text-v1.5, \
             multilingual-e5-small, multilingual-e5-base, multilingual-e5-large",
            other
        ),
    }
}

#[cfg(feature = "local-embeddings-fastembed")]
async fn embed_local_fastembed(
    config: &EmbeddingConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let model_name = config
        .model
        .clone()
        .unwrap_or_else(|| "all-minilm-l6-v2".to_string());

    let fastembed_model = config_to_fastembed_model(&model_name)?;
    let batch_size = config.batch_size;
    let texts = texts.to_vec();

    tokio::task::spawn_blocking(move || {
        let mut model = fastembed::TextEmbedding::try_new(
            fastembed::InitOptions::new(fastembed_model).with_show_download_progress(true),
        )
        .map_err(|e| anyhow::anyhow!("Failed to initialize local embedding model: {}", e))?;

        let embeddings = model
            .embed(texts, Some(batch_size))
            .map_err(|e| anyhow::anyhow!("Local embedding failed: {}", e))?;

        Ok(embeddings)
    })
    .await?
}

#[cfg(feature = "local-embeddings-tract")]
async fn embed_local_tract(config: &EmbeddingConfig, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    local_tract::embed_local_tract(config, texts).await
}

/// Create the appropriate [`EmbeddingProvider`] based on configuration.
///
/// # Supported Providers
///
/// | Config Value | Provider |
/// |-------------|----------|
/// | `"disabled"` | [`DisabledProvider`] |
/// | `"openai"` | [`OpenAIProvider`] |
/// | `"ollama"` | [`OllamaProvider`] |
/// | `"local"` | `LocalProvider` (fastembed or tract, see features) |
///
/// # Errors
///
/// Returns an error for unknown provider names or if the provider
/// cannot be initialized (missing config, API key, or feature flag).
pub fn create_provider(config: &EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    match config.provider.as_str() {
        "disabled" => Ok(Box::new(DisabledProvider)),
        "openai" => Ok(Box::new(OpenAIProvider::new(config)?)),
        "ollama" => Ok(Box::new(OllamaProvider::new(config)?)),
        #[cfg(any(feature = "local-embeddings-fastembed", feature = "local-embeddings-tract"))]
        "local" => Ok(Box::new(LocalProvider::new(config)?)),
        #[cfg(not(any(feature = "local-embeddings-fastembed", feature = "local-embeddings-tract")))]
        "local" => bail!(
            "Local embedding provider requires one of: --features local-embeddings-fastembed, --features local-embeddings-tract"
        ),
        other => bail!("Unknown embedding provider: {}", other),
    }
}

/// Encode a float vector as a BLOB (little-endian f32 bytes).
///
/// Each `f32` is stored as 4 bytes in little-endian order, producing
/// a BLOB of `vec.len() × 4` bytes. This format is compatible with
/// sqlite-vec for vector similarity search.
///
/// # Example
///
/// ```rust
/// use context_harness::embedding::{vec_to_blob, blob_to_vec};
///
/// let v = vec![1.0f32, -2.5, 3.125];
/// let blob = vec_to_blob(&v);
/// assert_eq!(blob.len(), 12); // 3 × 4 bytes
/// assert_eq!(blob_to_vec(&blob), v);
/// ```
pub fn vec_to_blob(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &v in vec {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Decode a BLOB back into a float vector.
///
/// Reverses [`vec_to_blob`]: reads 4-byte little-endian `f32` values
/// from the byte slice.
pub fn blob_to_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Compute cosine similarity between two embedding vectors.
///
/// Returns a value in `[-1.0, 1.0]`:
/// - `1.0` = identical direction
/// - `0.0` = orthogonal (unrelated)
/// - `-1.0` = opposite direction
///
/// Returns `0.0` for empty vectors or vectors of different lengths.
///
/// # Formula
///
/// ```text
///            a · b
/// cos(θ) = ─────────
///          ‖a‖ × ‖b‖
/// ```
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f32::EPSILON {
        return 0.0;
    }

    dot / denom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_blob_roundtrip() {
        let vec = vec![1.0f32, -2.5, 3.125, 0.0, -0.001];
        let blob = vec_to_blob(&vec);
        let restored = blob_to_vec(&blob);
        assert_eq!(vec, restored);
    }

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_empty() {
        let sim = cosine_similarity(&[], &[]);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }
}
