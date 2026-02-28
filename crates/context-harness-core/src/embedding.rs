//! Embedding provider trait and vector utilities.
//!
//! Defines the [`EmbeddingProvider`] trait that all embedding backends
//! implement, plus pure helper functions for vector serialization and
//! similarity computation.
//!
//! Concrete provider implementations (OpenAI, Ollama, fastembed, tract)
//! live in the `context-harness` app crate.

/// Trait for embedding providers.
///
/// Defines the interface that all embedding backends must implement.
/// Implementations are created by the application and passed to core
/// functions that need embedding metadata.
pub trait EmbeddingProvider: Send + Sync {
    /// Returns the model identifier (e.g. `"text-embedding-3-small"`).
    fn model_name(&self) -> &str;
    /// Returns the embedding vector dimensionality (e.g. `1536`).
    fn dims(&self) -> usize;
}

/// Encode a float vector as a BLOB (little-endian f32 bytes).
///
/// Each `f32` is stored as 4 bytes in little-endian order, producing
/// a BLOB of `vec.len() × 4` bytes.
///
/// # Example
///
/// ```rust
/// use context_harness_core::embedding::{vec_to_blob, blob_to_vec};
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
