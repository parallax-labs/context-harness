//! Tract-based local embedding pipeline (fallback for musl and Intel Mac).
//!
//! Pure-Rust path: loads ONNX model with tract-onnx, tokenizes with the tokenizers crate,
//! runs inference in spawn_blocking. No ONNX Runtime or system deps.
#![cfg_attr(
    all(feature = "local-embeddings-fastembed", feature = "local-embeddings-tract"),
    allow(dead_code)
)]

use anyhow::{bail, Result};
use std::path::PathBuf;
use tract_onnx::prelude::*;

use crate::config::EmbeddingConfig;

const ALL_MINILM_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";
const ALL_MINILM_DIMS: usize = 384;
const DEFAULT_MAX_LEN: usize = 256;

/// Model manifest: name -> (onnx path in repo, tokenizer path in repo, dims).
fn model_manifest(model_name: &str) -> Result<(&'static str, &'static str, usize)> {
    match model_name {
        "all-minilm-l6-v2" => Ok(("onnx/model.onnx", "tokenizer.json", ALL_MINILM_DIMS)),
        _ => bail!(
            "Tract backend supports only all-minilm-l6-v2 for now. Requested: '{}'",
            model_name
        ),
    }
}

fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(base)
        .join(".cache")
        .join("context-harness")
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| anyhow::anyhow!("Create cache dir: {}", e))?;
    Ok(dir)
}

fn download_to_cache(repo: &str, path: &str, cache_path: &std::path::Path) -> Result<()> {
    if cache_path.exists() {
        return Ok(());
    }
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        repo,
        path.replace(' ', "%20")
    );
    let resp = reqwest::blocking::get(&url)
        .map_err(|e| anyhow::anyhow!("Download {}: {}", url, e))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("Download {}: {}", url, e))?;
    let bytes = resp
        .bytes()
        .map_err(|e| anyhow::anyhow!("Read body: {}", e))?;
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("Create cache parent: {}", e))?;
    }
    std::fs::write(cache_path, &bytes).map_err(|e| anyhow::anyhow!("Write cache: {}", e))?;
    Ok(())
}

/// Ensure model and tokenizer are in cache; return (onnx path, tokenizer path).
fn ensure_cached(model_name: &str) -> Result<(PathBuf, PathBuf)> {
    let (onnx_rel, tokenizer_rel, _) = model_manifest(model_name)?;
    let dir = cache_dir()?;
    let model_dir = dir.join(model_name);
    let onnx_path = model_dir.join(onnx_rel);
    let tokenizer_path = model_dir.join(tokenizer_rel);
    download_to_cache(ALL_MINILM_REPO, onnx_rel, &onnx_path)?;
    download_to_cache(ALL_MINILM_REPO, tokenizer_rel, &tokenizer_path)?;
    Ok((onnx_path, tokenizer_path))
}

/// Run tract-based local embedding. Called from embedding.rs when `local-embeddings-tract` is enabled.
pub async fn embed_local_tract(
    config: &EmbeddingConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    let model_name = config
        .model
        .clone()
        .unwrap_or_else(|| "all-minilm-l6-v2".to_string());
    let batch_size = config.batch_size;
    let texts = texts.to_vec();

    tokio::task::spawn_blocking(move || run_tract_embed(&model_name, batch_size, &texts)).await?
}

fn run_tract_embed(model_name: &str, batch_size: usize, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let (_dims_name, dims) = model_manifest(model_name).map(|(_, _, d)| ((), d))?;
    let (onnx_path, tokenizer_path) = ensure_cached(model_name)?;

    let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path.to_str().unwrap())
        .map_err(|e| anyhow::anyhow!("Load tokenizer: {}", e))?;

    let model = tract_onnx::onnx()
        .model_for_path(onnx_path)
        .map_err(|e| anyhow::anyhow!("Load ONNX: {}", e))?
        .into_optimized()
        .map_err(|e| anyhow::anyhow!("Optimize: {}", e))?
        .into_runnable()
        .map_err(|e| anyhow::anyhow!("Build tract runnable: {}", e))?;

    let mut all_embeddings = Vec::with_capacity(texts.len());

    for chunk in texts.chunks(batch_size) {
        let encodings: Vec<_> = chunk
            .iter()
            .map(|s| {
                tokenizer
                    .encode(s.as_str(), true)
                    .map_err(|e| anyhow::anyhow!("Tokenize: {}", e))
            })
            .collect::<Result<Vec<_>>>()?;

        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(1)
            .min(DEFAULT_MAX_LEN);

        let batch_size_actual = encodings.len();
        let mut input_ids = vec![0i64; batch_size_actual * max_len];
        let mut attention_mask = vec![0i64; batch_size_actual * max_len];

        for (i, enc) in encodings.iter().enumerate() {
            let ids = enc.get_ids();
            let len = ids.len().min(max_len);
            for (j, &id) in ids.iter().take(len).enumerate() {
                input_ids[i * max_len + j] = id as i64;
                attention_mask[i * max_len + j] = 1;
            }
        }

        let input_ids_tensor =
            ndarray::Array2::from_shape_vec((batch_size_actual, max_len), input_ids)
                .map_err(|e| anyhow::anyhow!("Input ids shape: {}", e))?;
        let attention_mask_tensor =
            ndarray::Array2::from_shape_vec((batch_size_actual, max_len), attention_mask)
                .map_err(|e| anyhow::anyhow!("Attention mask shape: {}", e))?;

        let input_ids_t: Tensor = input_ids_tensor.into();
        let attention_mask_t: Tensor = attention_mask_tensor.into();
        let result = model.run(tvec!(input_ids_t.into(), attention_mask_t.into()))?;

        let output = result
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No output tensor"))?;
        let view = output
            .to_array_view::<f32>()
            .map_err(|e| anyhow::anyhow!("Output to array: {}", e))?;

        // Output shape is often [batch, seq_len, 384] (last_hidden_state); mean-pool over seq_len.
        // Or [batch, 384] if sentence_embedding. Handle both.
        let shape = view.shape();
        if shape.len() == 2 {
            for i in 0..shape[0] {
                let row = view.slice(ndarray::s![i, ..]);
                let vec: Vec<f32> = row.iter().copied().collect();
                all_embeddings.push(normalize_l2(vec));
            }
        } else if shape.len() == 3 {
            let seq_len = shape[1];
            for (i, enc) in encodings.iter().enumerate() {
                let valid_len = enc.get_ids().len().min(seq_len).min(max_len);
                let mut sum = vec![0f32; dims];
                let mut count = 0f32;
                for j in 0..valid_len {
                    for (k, &v) in view.slice(ndarray::s![i, j, ..]).iter().enumerate() {
                        if k < dims {
                            sum[k] += v;
                        }
                    }
                    count += 1.0;
                }
                if count > 0.0 {
                    for x in &mut sum {
                        *x /= count;
                    }
                }
                all_embeddings.push(normalize_l2(sum));
            }
        } else {
            bail!("Unexpected output shape: {:?}", shape);
        }
    }

    Ok(all_embeddings)
}

fn normalize_l2(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}
