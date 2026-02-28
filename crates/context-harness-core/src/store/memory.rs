//! In-memory [`Store`] implementation for testing and WASM targets.
//!
//! Uses `HashMap` and `Vec` behind `std::sync::RwLock` for thread safety.
//! Vector search is brute-force cosine similarity over all stored vectors.
//! Keyword search returns an empty result set (no FTS index).

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::Result;
use async_trait::async_trait;

use crate::models::{Chunk, Document};

use super::{ChunkCandidate, ChunkResponse, DocumentMetadata, DocumentResponse, Store};

struct StoredDoc {
    doc: Document,
    metadata_json_parsed: serde_json::Value,
}

struct StoredChunk {
    chunk: Chunk,
    document_id: String,
}

struct StoredVector {
    chunk_id: String,
    document_id: String,
    vector: Vec<f32>,
    _model: String,
    _dims: usize,
    _content_hash: String,
}

/// In-memory store for testing and WASM environments.
pub struct InMemoryStore {
    docs: RwLock<HashMap<String, StoredDoc>>,
    chunks: RwLock<Vec<StoredChunk>>,
    vectors: RwLock<Vec<StoredVector>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            docs: RwLock::new(HashMap::new()),
            chunks: RwLock::new(Vec::new()),
            vectors: RwLock::new(Vec::new()),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

fn format_ts_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a < f32::EPSILON || mag_b < f32::EPSILON {
        0.0
    } else {
        dot / (mag_a * mag_b)
    }
}

#[async_trait]
impl Store for InMemoryStore {
    async fn upsert_document(&self, doc: &Document) -> Result<String> {
        let parsed = serde_json::from_str(&doc.metadata_json).unwrap_or(serde_json::json!({}));
        let mut docs = self.docs.write().unwrap();
        docs.insert(
            doc.id.clone(),
            StoredDoc {
                doc: doc.clone(),
                metadata_json_parsed: parsed,
            },
        );
        Ok(doc.id.clone())
    }

    async fn replace_chunks(
        &self,
        doc_id: &str,
        chunks: &[Chunk],
        vectors: Option<&[Vec<f32>]>,
    ) -> Result<()> {
        {
            let mut stored = self.chunks.write().unwrap();
            stored.retain(|sc| sc.document_id != doc_id);
            for c in chunks {
                stored.push(StoredChunk {
                    chunk: c.clone(),
                    document_id: doc_id.to_string(),
                });
            }
        }
        if let Some(vecs) = vectors {
            let mut stored_vecs = self.vectors.write().unwrap();
            stored_vecs.retain(|sv| sv.document_id != doc_id);
            for (c, v) in chunks.iter().zip(vecs.iter()) {
                stored_vecs.push(StoredVector {
                    chunk_id: c.id.clone(),
                    document_id: doc_id.to_string(),
                    vector: v.clone(),
                    _model: String::new(),
                    _dims: v.len(),
                    _content_hash: c.hash.clone(),
                });
            }
        }
        Ok(())
    }

    async fn upsert_embedding(
        &self,
        chunk_id: &str,
        doc_id: &str,
        vector: &[f32],
        model: &str,
        dims: usize,
        content_hash: &str,
    ) -> Result<()> {
        let mut vecs = self.vectors.write().unwrap();
        vecs.retain(|sv| sv.chunk_id != chunk_id);
        vecs.push(StoredVector {
            chunk_id: chunk_id.to_string(),
            document_id: doc_id.to_string(),
            vector: vector.to_vec(),
            _model: model.to_string(),
            _dims: dims,
            _content_hash: content_hash.to_string(),
        });
        Ok(())
    }

    async fn get_document(&self, id: &str) -> Result<Option<DocumentResponse>> {
        let docs = self.docs.read().unwrap();
        let stored = match docs.get(id) {
            Some(s) => s,
            None => return Ok(None),
        };
        let chunks_guard = self.chunks.read().unwrap();
        let mut chunk_responses: Vec<ChunkResponse> = chunks_guard
            .iter()
            .filter(|sc| sc.document_id == id)
            .map(|sc| ChunkResponse {
                index: sc.chunk.chunk_index,
                text: sc.chunk.text.clone(),
            })
            .collect();
        chunk_responses.sort_by_key(|c| c.index);

        Ok(Some(DocumentResponse {
            id: stored.doc.id.clone(),
            source: stored.doc.source.clone(),
            source_id: stored.doc.source_id.clone(),
            source_url: stored.doc.source_url.clone(),
            title: stored.doc.title.clone(),
            author: stored.doc.author.clone(),
            created_at: format_ts_iso(stored.doc.created_at),
            updated_at: format_ts_iso(stored.doc.updated_at),
            content_type: stored.doc.content_type.clone(),
            body: stored.doc.body.clone(),
            metadata: stored.metadata_json_parsed.clone(),
            chunks: chunk_responses,
        }))
    }

    async fn get_document_metadata(&self, id: &str) -> Result<Option<DocumentMetadata>> {
        let docs = self.docs.read().unwrap();
        Ok(docs.get(id).map(|s| DocumentMetadata {
            id: s.doc.id.clone(),
            title: s.doc.title.clone(),
            source: s.doc.source.clone(),
            source_id: s.doc.source_id.clone(),
            source_url: s.doc.source_url.clone(),
            updated_at: s.doc.updated_at,
        }))
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
        _source: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let chunks_guard = self.chunks.read().unwrap();
        let mut candidates: Vec<ChunkCandidate> = chunks_guard
            .iter()
            .filter_map(|sc| {
                let text_lower = sc.chunk.text.to_lowercase();
                let matches: usize = terms.iter().filter(|t| text_lower.contains(*t)).count();
                if matches > 0 {
                    let snippet = sc.chunk.text.chars().take(240).collect::<String>();
                    Some(ChunkCandidate {
                        chunk_id: sc.chunk.id.clone(),
                        document_id: sc.document_id.clone(),
                        raw_score: matches as f64,
                        snippet,
                    })
                } else {
                    None
                }
            })
            .collect();
        candidates.sort_by(|a, b| {
            b.raw_score
                .partial_cmp(&a.raw_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limit as usize);
        Ok(candidates)
    }

    async fn vector_search(
        &self,
        query_vec: &[f32],
        limit: i64,
        _source: Option<&str>,
        _since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        let vecs = self.vectors.read().unwrap();
        let chunks_guard = self.chunks.read().unwrap();
        let mut candidates: Vec<ChunkCandidate> = vecs
            .iter()
            .map(|sv| {
                let sim = cosine_sim(query_vec, &sv.vector) as f64;
                let snippet = chunks_guard
                    .iter()
                    .find(|sc| sc.chunk.id == sv.chunk_id)
                    .map(|sc| sc.chunk.text.chars().take(240).collect::<String>())
                    .unwrap_or_default();
                ChunkCandidate {
                    chunk_id: sv.chunk_id.clone(),
                    document_id: sv.document_id.clone(),
                    raw_score: sim,
                    snippet,
                }
            })
            .collect();
        candidates.sort_by(|a, b| {
            b.raw_score
                .partial_cmp(&a.raw_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limit as usize);
        Ok(candidates)
    }
}
