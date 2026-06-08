//! Optional vector-index boundary and production zvec sidecar support.
//!
//! SQLite remains the canonical store for documents, chunks, FTS5 rows, and
//! embedding metadata. A [`VectorIndex`] only retrieves vector candidates for
//! semantic search; core hybrid scoring still consumes [`ChunkCandidate`]s.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use context_harness_core::embedding::blob_to_vec;
use context_harness_core::models::{Chunk, Document};
use context_harness_core::store::{ChunkCandidate, DocumentMetadata, DocumentResponse, Store};

use crate::config::Config;
use crate::ctx_dirs;
use crate::sqlite_store::SqliteStore;

#[allow(dead_code)]
const MANIFEST_VERSION: u32 = 1;
#[allow(dead_code)]
const COLLECTION_DIR: &str = "collection";
#[allow(dead_code)]
const MANIFEST_FILE: &str = "manifest.json";
#[allow(dead_code)]
const ZVEC_BATCH_SIZE: usize = 512;

/// A vector row available to an optional vector index.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VectorRecord {
    pub chunk_id: String,
    pub document_id: String,
    pub vector: Vec<f32>,
    pub model: String,
    pub dims: usize,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct IndexedVectorRecord {
    record: VectorRecord,
    snippet: String,
    source: String,
    updated_at: i64,
}

/// Search controls passed to vector-index backends.
#[derive(Debug, Clone, Copy, Default)]
pub struct VectorSearchOptions<'a> {
    pub limit: i64,
    pub source: Option<&'a str>,
    pub since: Option<&'a str>,
}

/// Health information for the configured vector-index backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexHealth {
    pub enabled: bool,
    pub available: bool,
    pub backend: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorIndexManifest {
    pub version: u32,
    pub backend: String,
    pub vector_count: usize,
    pub model: Option<String>,
    pub dims: Option<usize>,
    pub metric: String,
    pub index: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndexStatus {
    pub path: PathBuf,
    pub health: VectorIndexHealth,
    pub manifest: Option<VectorIndexManifest>,
    pub sqlite_vector_count: usize,
    pub sqlite_digest: String,
    pub fresh: bool,
}

/// Optional semantic-search accelerator.
#[async_trait]
pub trait VectorIndex: Send + Sync {
    #[allow(dead_code)]
    async fn upsert(&self, record: &VectorRecord) -> Result<()>;
    #[allow(dead_code)]
    async fn delete_chunk(&self, chunk_id: &str) -> Result<()>;
    #[allow(dead_code)]
    async fn delete_document(&self, document_id: &str) -> Result<()>;
    async fn search(
        &self,
        query_vec: &[f32],
        options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>>;
    #[allow(dead_code)]
    async fn health(&self) -> Result<VectorIndexHealth>;
}

/// Disabled vector index. Callers should use the configured fallback.
pub struct DisabledVectorIndex;

#[async_trait]
impl VectorIndex for DisabledVectorIndex {
    async fn upsert(&self, _record: &VectorRecord) -> Result<()> {
        Ok(())
    }

    async fn delete_chunk(&self, _chunk_id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete_document(&self, _document_id: &str) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        _query_vec: &[f32],
        _options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>> {
        Ok(Vec::new())
    }

    async fn health(&self) -> Result<VectorIndexHealth> {
        Ok(VectorIndexHealth {
            enabled: false,
            available: false,
            backend: "disabled".to_string(),
            message: Some("vector index disabled; SQLite fallback remains canonical".to_string()),
        })
    }
}

/// Exact brute-force SQLite vector scan used as the behavioral baseline.
pub struct BruteForceSqliteVectorIndex {
    store: SqliteStore,
}

impl BruteForceSqliteVectorIndex {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl VectorIndex for BruteForceSqliteVectorIndex {
    async fn upsert(&self, _record: &VectorRecord) -> Result<()> {
        Ok(())
    }

    async fn delete_chunk(&self, _chunk_id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete_document(&self, _document_id: &str) -> Result<()> {
        Ok(())
    }

    async fn search(
        &self,
        query_vec: &[f32],
        options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>> {
        self.store
            .vector_search(query_vec, options.limit, options.source, options.since)
            .await
    }

    async fn health(&self) -> Result<VectorIndexHealth> {
        Ok(VectorIndexHealth {
            enabled: true,
            available: true,
            backend: "sqlite-bruteforce".to_string(),
            message: Some("exact SQLite vector scan".to_string()),
        })
    }
}

pub struct VectorIndexRouter {
    primary: Option<Arc<dyn VectorIndex>>,
    fallback: Option<BruteForceSqliteVectorIndex>,
    backend: String,
}

impl VectorIndexRouter {
    async fn search(
        &self,
        query_vec: &[f32],
        options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>> {
        if let Some(primary) = &self.primary {
            match primary.search(query_vec, options).await {
                Ok(candidates) => return Ok(candidates),
                Err(err) if self.fallback.is_some() => {
                    eprintln!(
                        "Warning: vector index '{}' failed; falling back to SQLite: {}",
                        self.backend, err
                    );
                }
                Err(err) => return Err(err),
            }
        }

        if let Some(fallback) = &self.fallback {
            return fallback.search(query_vec, options).await;
        }

        Ok(Vec::new())
    }

    #[allow(dead_code)]
    pub async fn health(&self) -> Result<VectorIndexHealth> {
        if let Some(primary) = &self.primary {
            return primary.health().await;
        }
        if let Some(fallback) = &self.fallback {
            return fallback.health().await;
        }
        DisabledVectorIndex.health().await
    }
}

/// Store wrapper that preserves all SQLite behavior except vector candidates.
pub struct VectorIndexedStore {
    sqlite: SqliteStore,
    router: VectorIndexRouter,
}

impl VectorIndexedStore {
    pub fn new(sqlite: SqliteStore, router: VectorIndexRouter) -> Self {
        Self { sqlite, router }
    }
}

#[async_trait]
impl Store for VectorIndexedStore {
    async fn upsert_document(&self, doc: &Document) -> Result<String> {
        self.sqlite.upsert_document(doc).await
    }

    async fn replace_chunks(
        &self,
        doc_id: &str,
        chunks: &[Chunk],
        vectors: Option<&[Vec<f32>]>,
    ) -> Result<()> {
        self.sqlite.replace_chunks(doc_id, chunks, vectors).await
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
        self.sqlite
            .upsert_embedding(chunk_id, doc_id, vector, model, dims, content_hash)
            .await
    }

    async fn get_document(&self, id: &str) -> Result<Option<DocumentResponse>> {
        self.sqlite.get_document(id).await
    }

    async fn get_document_metadata(&self, id: &str) -> Result<Option<DocumentMetadata>> {
        self.sqlite.get_document_metadata(id).await
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
        source: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        self.sqlite
            .keyword_search(query, limit, source, since)
            .await
    }

    async fn vector_search(
        &self,
        query_vec: &[f32],
        limit: i64,
        source: Option<&str>,
        since: Option<&str>,
    ) -> Result<Vec<ChunkCandidate>> {
        self.router
            .search(
                query_vec,
                VectorSearchOptions {
                    limit,
                    source,
                    since,
                },
            )
            .await
    }
}

pub async fn configured_vector_store(
    config: &Config,
    pool: SqlitePool,
) -> Result<VectorIndexedStore> {
    let sqlite = SqliteStore::new(pool.clone());
    let router = configured_vector_index(config, pool).await?;
    Ok(VectorIndexedStore::new(sqlite, router))
}

pub async fn configured_vector_index(
    config: &Config,
    pool: SqlitePool,
) -> Result<VectorIndexRouter> {
    let backend = config.vector_index.backend.as_str();
    let sqlite_fallback = || BruteForceSqliteVectorIndex::new(SqliteStore::new(pool.clone()));
    let fallback = match config.vector_index.fallback.as_str() {
        "sqlite" => Some(sqlite_fallback()),
        _ => None,
    };

    match backend {
        "sqlite" => Ok(VectorIndexRouter {
            primary: None,
            fallback: Some(sqlite_fallback()),
            backend: "sqlite".to_string(),
        }),
        "disabled" => Ok(VectorIndexRouter {
            primary: None,
            fallback,
            backend: "disabled".to_string(),
        }),
        "auto" => configured_auto_zvec(config, pool.clone(), fallback).await,
        "zvec" => configured_required_zvec(config, pool.clone(), fallback).await,
        other => Err(anyhow!("unknown vector_index.backend: {other}")),
    }
}

#[cfg(feature = "zvec-bundled")]
async fn configured_auto_zvec(
    config: &Config,
    pool: SqlitePool,
    fallback: Option<BruteForceSqliteVectorIndex>,
) -> Result<VectorIndexRouter> {
    match ZvecVectorIndex::open_or_rebuild(config, pool).await {
        Ok(index) => Ok(VectorIndexRouter {
            primary: Some(Arc::new(index)),
            fallback,
            backend: "zvec".to_string(),
        }),
        Err(err) => {
            eprintln!("Warning: zvec unavailable; using SQLite vector fallback: {err}");
            Ok(VectorIndexRouter {
                primary: None,
                fallback,
                backend: "auto".to_string(),
            })
        }
    }
}

#[cfg(not(feature = "zvec-bundled"))]
async fn configured_auto_zvec(
    _config: &Config,
    _pool: SqlitePool,
    fallback: Option<BruteForceSqliteVectorIndex>,
) -> Result<VectorIndexRouter> {
    Ok(VectorIndexRouter {
        primary: None,
        fallback,
        backend: "auto".to_string(),
    })
}

#[cfg(feature = "zvec-bundled")]
async fn configured_required_zvec(
    config: &Config,
    pool: SqlitePool,
    fallback: Option<BruteForceSqliteVectorIndex>,
) -> Result<VectorIndexRouter> {
    let index = ZvecVectorIndex::open_or_rebuild(config, pool).await?;
    Ok(VectorIndexRouter {
        primary: Some(Arc::new(index)),
        fallback,
        backend: "zvec".to_string(),
    })
}

#[cfg(not(feature = "zvec-bundled"))]
async fn configured_required_zvec(
    _config: &Config,
    _pool: SqlitePool,
    _fallback: Option<BruteForceSqliteVectorIndex>,
) -> Result<VectorIndexRouter> {
    Err(anyhow!(
        "vector_index.backend = 'zvec' requires building with --features zvec-bundled"
    ))
}

pub fn resolve_vector_index_path(config: &Config) -> PathBuf {
    if config.vector_index.path != PathBuf::from("auto") {
        return config.vector_index.path.clone();
    }

    if ctx_dirs::is_default_workspace_db_path(&config.db.path) {
        return ctx_dirs::workspace_vector_index_dir();
    }

    config
        .db
        .path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("vector-index")
        .join("zvec")
}

pub async fn vector_index_status(config: &Config) -> Result<VectorIndexStatus> {
    let pool = crate::db::connect(config).await?;
    let path = resolve_vector_index_path(config);
    let manifest = read_manifest(&path)?;
    let snapshot = sqlite_vector_snapshot(&pool).await?;
    pool.close().await;

    let fresh = manifest
        .as_ref()
        .is_some_and(|manifest| manifest.digest == snapshot.digest);
    let health = status_health(config, manifest.as_ref(), fresh, snapshot.records.len());
    Ok(VectorIndexStatus {
        path,
        health,
        manifest,
        sqlite_vector_count: snapshot.records.len(),
        sqlite_digest: snapshot.digest,
        fresh,
    })
}

fn status_health(
    config: &Config,
    manifest: Option<&VectorIndexManifest>,
    fresh: bool,
    sqlite_vector_count: usize,
) -> VectorIndexHealth {
    match config.vector_index.backend.as_str() {
        "sqlite" => VectorIndexHealth {
            enabled: true,
            available: true,
            backend: "sqlite-bruteforce".to_string(),
            message: Some("exact SQLite vector scan".to_string()),
        },
        "disabled" => VectorIndexHealth {
            enabled: false,
            available: false,
            backend: "disabled".to_string(),
            message: Some("vector index disabled; SQLite fallback remains canonical".to_string()),
        },
        "zvec" | "auto" if cfg!(feature = "zvec-bundled") => VectorIndexHealth {
            enabled: true,
            available: fresh,
            backend: "zvec".to_string(),
            message: Some(if sqlite_vector_count == 0 {
                "SQLite has no vectors to index".to_string()
            } else if manifest.is_none() {
                "zvec sidecar missing".to_string()
            } else if fresh {
                "zvec sidecar fresh".to_string()
            } else {
                "zvec sidecar stale".to_string()
            }),
        },
        _ => VectorIndexHealth {
            enabled: true,
            available: config.vector_index.fallback == "sqlite",
            backend: "sqlite-bruteforce".to_string(),
            message: Some("zvec not compiled in; using SQLite fallback".to_string()),
        },
    }
}

pub async fn rebuild_configured_vector_index(config: &Config) -> Result<VectorIndexStatus> {
    let pool = crate::db::connect(config).await?;
    rebuild_zvec_if_available(config, &pool).await?;
    pool.close().await;
    vector_index_status(config).await
}

#[cfg(feature = "zvec-bundled")]
async fn rebuild_zvec_if_available(config: &Config, pool: &SqlitePool) -> Result<()> {
    ZvecVectorIndex::rebuild(config, pool.clone())
        .await
        .map(|_| ())
}

#[cfg(not(feature = "zvec-bundled"))]
async fn rebuild_zvec_if_available(config: &Config, _pool: &SqlitePool) -> Result<()> {
    if config.vector_index.backend == "zvec" {
        Err(anyhow!(
            "vector_index.backend = 'zvec' requires building with --features zvec-bundled"
        ))
    } else {
        Ok(())
    }
}

pub async fn sync_vector_record_after_sqlite(
    config: &Config,
    pool: &SqlitePool,
    record: &VectorRecord,
) -> Result<()> {
    sync_vector_record_after_sqlite_impl(config, pool, record).await
}

#[cfg(feature = "zvec-bundled")]
async fn sync_vector_record_after_sqlite_impl(
    config: &Config,
    pool: &SqlitePool,
    record: &VectorRecord,
) -> Result<()> {
    if matches!(config.vector_index.backend.as_str(), "sqlite" | "disabled") {
        return Ok(());
    }

    let path = resolve_vector_index_path(config);
    let Some(manifest) = read_manifest(&path)? else {
        return Ok(());
    };
    if manifest.dims != Some(record.dims) {
        return Ok(());
    }

    let index = ZvecVectorIndex::open_existing(config, pool.clone()).await?;
    index.upsert(record).await?;
    index.write_current_manifest().await?;
    Ok(())
}

#[cfg(not(feature = "zvec-bundled"))]
async fn sync_vector_record_after_sqlite_impl(
    _config: &Config,
    _pool: &SqlitePool,
    _record: &VectorRecord,
) -> Result<()> {
    Ok(())
}

pub fn remove_configured_sidecar(config: &Config) -> Result<()> {
    if matches!(config.vector_index.backend.as_str(), "sqlite" | "disabled") {
        return Ok(());
    }

    let path = resolve_vector_index_path(config);
    if path.is_dir() {
        std::fs::remove_dir_all(&path)
            .with_context(|| format!("failed to remove vector index sidecar {}", path.display()))?;
    } else if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove vector index sidecar {}", path.display()))?;
    }
    Ok(())
}

struct SqliteVectorSnapshot {
    records: Vec<IndexedVectorRecord>,
    digest: String,
}

async fn sqlite_vector_snapshot(pool: &SqlitePool) -> Result<SqliteVectorSnapshot> {
    let rows = sqlx::query(
        r#"
        SELECT cv.chunk_id, cv.document_id, cv.embedding,
               e.model, e.dims, e.hash,
               COALESCE(substr(c.text, 1, 240), '') AS snippet,
               d.source, d.updated_at
        FROM chunk_vectors cv
        JOIN embeddings e ON e.chunk_id = cv.chunk_id
        JOIN chunks c ON c.id = cv.chunk_id
        JOIN documents d ON d.id = cv.document_id
        ORDER BY cv.chunk_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut hasher = Sha256::new();
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        let chunk_id: String = row.get("chunk_id");
        let document_id: String = row.get("document_id");
        let blob: Vec<u8> = row.get("embedding");
        let model: String = row.get("model");
        let dims: i64 = row.get("dims");
        let content_hash: String = row.get("hash");
        let source: String = row.get("source");
        let updated_at: i64 = row.get("updated_at");

        hasher.update(chunk_id.as_bytes());
        hasher.update([0]);
        hasher.update(document_id.as_bytes());
        hasher.update([0]);
        hasher.update(model.as_bytes());
        hasher.update([0]);
        hasher.update(dims.to_le_bytes());
        hasher.update(content_hash.as_bytes());
        hasher.update([0]);
        hasher.update(&blob);

        records.push(IndexedVectorRecord {
            record: VectorRecord {
                chunk_id,
                document_id,
                vector: blob_to_vec(&blob),
                model,
                dims: dims as usize,
                content_hash,
            },
            snippet: row.get("snippet"),
            source,
            updated_at,
        });
    }

    Ok(SqliteVectorSnapshot {
        records,
        digest: hex::encode(hasher.finalize()),
    })
}

#[allow(dead_code)]
fn manifest_for_snapshot(snapshot: &SqliteVectorSnapshot, config: &Config) -> VectorIndexManifest {
    let first = snapshot.records.first();
    VectorIndexManifest {
        version: MANIFEST_VERSION,
        backend: "zvec".to_string(),
        vector_count: snapshot.records.len(),
        model: first.map(|r| r.record.model.clone()),
        dims: first.map(|r| r.record.dims),
        metric: config.vector_index.metric.clone(),
        index: config.vector_index.index.clone(),
        digest: snapshot.digest.clone(),
    }
}

fn read_manifest(path: &Path) -> Result<Option<VectorIndexManifest>> {
    let manifest_path = path.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))
        .map(Some)
}

#[allow(dead_code)]
fn write_manifest(path: &Path, manifest: &VectorIndexManifest) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let content = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path.join(MANIFEST_FILE), content)?;
    Ok(())
}

#[allow(dead_code)]
fn parse_since(since: Option<&str>) -> Result<Option<i64>> {
    let Some(since) = since else {
        return Ok(None);
    };
    let date = chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d")?;
    Ok(Some(
        date.and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("invalid since date"))?
            .and_utc()
            .timestamp(),
    ))
}

#[cfg(feature = "zvec-bundled")]
pub struct ZvecVectorIndex {
    config: Config,
    pool: SqlitePool,
    path: PathBuf,
    collection: zvec::Collection,
}

#[cfg(feature = "zvec-bundled")]
impl ZvecVectorIndex {
    pub async fn open_or_rebuild(config: &Config, pool: SqlitePool) -> Result<Self> {
        let path = resolve_vector_index_path(config);
        let snapshot = sqlite_vector_snapshot(&pool).await?;
        let manifest = read_manifest(&path)?;
        let expected = manifest_for_snapshot(&snapshot, config);
        let collection_path = path.join(COLLECTION_DIR);

        if snapshot.records.is_empty() {
            return Err(anyhow!("SQLite has no vectors to index"));
        }

        if manifest.as_ref().is_some_and(|m| m == &expected) && collection_path.exists() {
            return Self::open_existing(config, pool).await;
        }

        Self::rebuild_from_snapshot(config, pool, path, snapshot).await
    }

    async fn open_existing(config: &Config, pool: SqlitePool) -> Result<Self> {
        let path = resolve_vector_index_path(config);
        let collection_path = path.join(COLLECTION_DIR);
        let collection = zvec::Collection::open(collection_path.to_string_lossy().as_ref(), None)
            .with_context(|| {
            format!(
                "failed to open zvec collection {}",
                collection_path.display()
            )
        })?;
        Ok(Self {
            config: config.clone(),
            pool,
            path,
            collection,
        })
    }

    pub async fn rebuild(config: &Config, pool: SqlitePool) -> Result<Self> {
        let path = resolve_vector_index_path(config);
        let snapshot = sqlite_vector_snapshot(&pool).await?;
        Self::rebuild_from_snapshot(config, pool, path, snapshot).await
    }

    async fn rebuild_from_snapshot(
        config: &Config,
        pool: SqlitePool,
        path: PathBuf,
        snapshot: SqliteVectorSnapshot,
    ) -> Result<Self> {
        if path.exists() {
            std::fs::remove_dir_all(&path).with_context(|| {
                format!("failed to remove stale zvec sidecar {}", path.display())
            })?;
        }
        std::fs::create_dir_all(&path)?;

        let dims = snapshot
            .records
            .first()
            .map(|r| r.record.dims)
            .ok_or_else(|| anyhow!("SQLite has no vectors to index"))?;
        let schema = zvec_schema(dims, &config.vector_index.index)?;
        let collection_path = path.join(COLLECTION_DIR);
        let collection = zvec::Collection::create_and_open(
            collection_path.to_string_lossy().as_ref(),
            &schema,
            None,
        )
        .with_context(|| {
            format!(
                "failed to create zvec collection {}",
                collection_path.display()
            )
        })?;

        let index = Self {
            config: config.clone(),
            pool,
            path: path.clone(),
            collection,
        };
        index.populate(&snapshot.records)?;
        index.collection.optimize()?;
        index.collection.flush()?;
        write_manifest(&path, &manifest_for_snapshot(&snapshot, config))?;
        Ok(index)
    }

    fn populate(&self, records: &[IndexedVectorRecord]) -> Result<()> {
        let mut batch = Vec::with_capacity(ZVEC_BATCH_SIZE);
        for record in records {
            batch.push(doc_from_record(record)?);
            if batch.len() == ZVEC_BATCH_SIZE {
                self.upsert_docs(&batch)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            self.upsert_docs(&batch)?;
        }
        self.collection.flush()?;
        Ok(())
    }

    fn upsert_docs(&self, docs: &[zvec::Doc]) -> Result<()> {
        let refs: Vec<&zvec::Doc> = docs.iter().collect();
        self.collection.upsert(&refs)?;
        Ok(())
    }

    async fn write_current_manifest(&self) -> Result<()> {
        let snapshot = sqlite_vector_snapshot(&self.pool).await?;
        write_manifest(&self.path, &manifest_for_snapshot(&snapshot, &self.config))
    }
}

#[cfg(feature = "zvec-bundled")]
#[async_trait]
impl VectorIndex for ZvecVectorIndex {
    async fn upsert(&self, record: &VectorRecord) -> Result<()> {
        let row = sqlx::query(
            r#"
            SELECT COALESCE(substr(c.text, 1, 240), '') AS snippet,
                   d.source, d.updated_at
            FROM chunks c
            JOIN documents d ON d.id = c.document_id
            WHERE c.id = ?
            "#,
        )
        .bind(&record.chunk_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(());
        };

        let indexed = IndexedVectorRecord {
            record: record.clone(),
            snippet: row.get("snippet"),
            source: row.get("source"),
            updated_at: row.get("updated_at"),
        };
        let doc = doc_from_record(&indexed)?;
        self.upsert_docs(&[doc])?;
        self.collection.flush()?;
        Ok(())
    }

    async fn delete_chunk(&self, chunk_id: &str) -> Result<()> {
        self.collection.delete(&[chunk_id])?;
        self.collection.flush()?;
        self.write_current_manifest().await?;
        Ok(())
    }

    async fn delete_document(&self, document_id: &str) -> Result<()> {
        let escaped = document_id.replace('\'', "\\'");
        self.collection
            .delete_by_filter(&format!("document_id == '{escaped}'"))?;
        self.collection.flush()?;
        self.write_current_manifest().await?;
        Ok(())
    }

    async fn search(
        &self,
        query_vec: &[f32],
        options: VectorSearchOptions<'_>,
    ) -> Result<Vec<ChunkCandidate>> {
        let limit = options.limit.max(0) as usize;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let since_ts = parse_since(options.since)?;
        let topk = (limit * 8).max(limit).min(i32::MAX as usize) as i32;
        let query = zvec::VectorQuery::builder()
            .field("embedding")
            .vector_fp32(query_vec)
            .topk(topk)
            .build()?;

        let rows = self.collection.query(&query)?;
        let mut candidates = Vec::new();
        for row in rows.iter() {
            let Some(chunk_id) = row.pk_copy() else {
                continue;
            };
            let source = row.get_string("source")?.unwrap_or_default();
            if options.source.is_some_and(|expected| source != expected) {
                continue;
            }
            let updated_at = row.get_int64("updated_at")?;
            if since_ts.is_some_and(|since| updated_at < since) {
                continue;
            }
            candidates.push(ChunkCandidate {
                chunk_id,
                document_id: row.get_string("document_id")?.unwrap_or_default(),
                raw_score: 1.0 - row.score() as f64,
                snippet: row.get_string("snippet")?.unwrap_or_default(),
            });
            if candidates.len() == limit {
                break;
            }
        }
        Ok(candidates)
    }

    async fn health(&self) -> Result<VectorIndexHealth> {
        let snapshot = sqlite_vector_snapshot(&self.pool).await?;
        let manifest = read_manifest(&self.path)?;
        let fresh = manifest
            .as_ref()
            .is_some_and(|manifest| manifest.digest == snapshot.digest);
        Ok(VectorIndexHealth {
            enabled: true,
            available: fresh,
            backend: "zvec".to_string(),
            message: Some(if fresh {
                "zvec sidecar fresh".to_string()
            } else {
                "zvec sidecar stale".to_string()
            }),
        })
    }
}

#[cfg(feature = "zvec-bundled")]
fn zvec_schema(dims: usize, index: &str) -> Result<zvec::CollectionSchema> {
    let vector = match index {
        "flat" => zvec::FieldSchema::vector_fp32("embedding", dims as u32)
            .flat()
            .metric(zvec::MetricType::Cosine),
        _ => zvec::FieldSchema::vector_fp32("embedding", dims as u32)
            .hnsw(16, 200)
            .metric(zvec::MetricType::Cosine),
    };

    Ok(zvec::CollectionSchema::builder("context_chunks")
        .field(zvec::FieldSchema::string("document_id").invert_index(true, false))
        .field(zvec::FieldSchema::string("source").invert_index(true, false))
        .field(zvec::FieldSchema::int64("updated_at").invert_index(true, false))
        .field(zvec::FieldSchema::string("snippet"))
        .field(vector)
        .build()?)
}

#[cfg(feature = "zvec-bundled")]
fn doc_from_record(record: &IndexedVectorRecord) -> Result<zvec::Doc> {
    let mut doc = zvec::Doc::new()?;
    doc.set_pk(&record.record.chunk_id)?;
    doc.add_string("document_id", &record.record.document_id)?;
    doc.add_string("source", &record.source)?;
    doc.add_int64("updated_at", record.updated_at)?;
    doc.add_string("snippet", &record.snippet)?;
    doc.add_vector_fp32("embedding", &record.record.vector)?;
    Ok(doc)
}
