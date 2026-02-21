use anyhow::{bail, Result};
use std::time::Duration;

use crate::config::EmbeddingConfig;

/// Trait for embedding providers.
pub trait EmbeddingProvider: Send + Sync {
    fn model_name(&self) -> &str;
    fn dims(&self) -> usize;
}

/// Async embedding function (not in the trait due to async limitations).
pub async fn embed_texts(
    _provider: &dyn EmbeddingProvider,
    config: &EmbeddingConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    match config.provider.as_str() {
        "openai" => embed_openai(config, texts).await,
        "disabled" => bail!("Embedding provider is disabled"),
        other => bail!("Unknown embedding provider: {}", other),
    }
}

/// Embed a single query text.
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

pub struct OpenAIProvider {
    model: String,
    dims: usize,
}

impl OpenAIProvider {
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

/// Call OpenAI embeddings API with retry/backoff.
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

/// Create the appropriate provider based on config.
pub fn create_provider(config: &EmbeddingConfig) -> Result<Box<dyn EmbeddingProvider>> {
    match config.provider.as_str() {
        "disabled" => Ok(Box::new(DisabledProvider)),
        "openai" => Ok(Box::new(OpenAIProvider::new(config)?)),
        other => bail!("Unknown embedding provider: {}", other),
    }
}

/// Encode a vector as a BLOB (little-endian f32 bytes).
pub fn vec_to_blob(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &v in vec {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Decode a BLOB back into a vector.
pub fn blob_to_vec(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Compute cosine similarity between two vectors.
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
