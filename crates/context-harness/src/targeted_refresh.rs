//! Trusted, bounded refresh of explicit items without connector discovery.

use crate::{
    app_store::{hash_text, AppStore, SqliteAppStore},
    config::Config,
    document_ingestor::{prepare_source_item, DocumentPreparationError},
    embedding,
    models::{Chunk, SourceItem},
};
use anyhow::{bail, Context, Result};
use context_harness_core::store::Store;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

pub const MAX_TARGETED_REFRESH_ITEMS: usize = 100;
pub const MAX_TARGETED_REFRESH_PAYLOAD_BYTES: usize = 50_000_000;
const DEFAULT_MAX_EXTRACT_BYTES: u64 = 50_000_000;

#[derive(Debug)]
enum SourcePolicy {
    Paths {
        include: GlobSet,
        exclude: GlobSet,
    },
    S3 {
        prefix: String,
        include: GlobSet,
        exclude: GlobSet,
    },
    Script,
}

/// Opaque authority for one configured source in one canonical workspace.
#[derive(Debug)]
pub struct EnrolledSource {
    workspace: PathBuf,
    source_label: String,
    max_extract_bytes: u64,
    policy: SourcePolicy,
}

impl EnrolledSource {
    /// Resolve configured source authority without opening storage or executing a connector.
    pub fn resolve(config: &Config, workspace: &Path, source_label: &str) -> Result<Self> {
        let workspace = workspace
            .canonicalize()
            .context("canonicalizing targeted-refresh workspace")?;
        let (max_extract_bytes, policy) =
            if let Some(name) = source_label.strip_prefix("filesystem:") {
                let source = config
                    .connectors
                    .filesystem
                    .get(name)
                    .context("configured targeted-refresh source not found")?;
                (
                    source.max_extract_bytes,
                    SourcePolicy::Paths {
                        include: globset(&source.include_globs)?,
                        exclude: path_excludes(&source.exclude_globs)?,
                    },
                )
            } else if let Some(name) = source_label.strip_prefix("git:") {
                let source = config
                    .connectors
                    .git
                    .get(name)
                    .context("configured targeted-refresh source not found")?;
                (
                    DEFAULT_MAX_EXTRACT_BYTES,
                    SourcePolicy::Paths {
                        include: globset(&source.include_globs)?,
                        exclude: path_excludes(&source.exclude_globs)?,
                    },
                )
            } else if let Some(name) = source_label.strip_prefix("s3:") {
                let source = config
                    .connectors
                    .s3
                    .get(name)
                    .context("configured targeted-refresh source not found")?;
                (
                    DEFAULT_MAX_EXTRACT_BYTES,
                    SourcePolicy::S3 {
                        prefix: source.prefix.clone(),
                        include: globset(&source.include_globs)?,
                        exclude: s3_excludes(&source.exclude_globs)?,
                    },
                )
            } else if let Some(name) = source_label.strip_prefix("script:") {
                config
                    .connectors
                    .script
                    .get(name)
                    .context("configured targeted-refresh source not found")?;
                (DEFAULT_MAX_EXTRACT_BYTES, SourcePolicy::Script)
            } else {
                bail!("configured targeted-refresh source not found");
            };
        Ok(Self {
            workspace,
            source_label: source_label.to_owned(),
            max_extract_bytes,
            policy,
        })
    }

    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    fn accepts(&self, source_id: &str) -> bool {
        match &self.policy {
            SourcePolicy::Paths { include, exclude } => normalize_relative(source_id)
                .is_some_and(|path| include.is_match(&path) && !exclude.is_match(&path)),
            SourcePolicy::S3 {
                prefix,
                include,
                exclude,
            } => {
                let relative = if prefix.is_empty() {
                    source_id
                } else {
                    let normalized = prefix.trim_end_matches('/');
                    let Some(rest) = source_id.strip_prefix(normalized) else {
                        return false;
                    };
                    if !rest.starts_with('/') {
                        return false;
                    }
                    rest.trim_start_matches('/')
                };
                !relative.is_empty() && include.is_match(relative) && !exclude.is_match(relative)
            }
            SourcePolicy::Script => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalRefreshStatus {
    Indexed,
    Unchanged,
    Rejected,
    NotAttempted,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedRefreshStatus {
    NotConfigured,
    Current,
    Pending,
    Failed,
    NotAttempted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    EmptyBatch,
    TooManyItems,
    PayloadTooLarge,
    WorkspaceMismatch,
    SourceMismatch,
    EmptySourceId,
    EmptyContentType,
    InvalidMetadata,
    RawBodyConflict,
    ItemTooLarge,
    SourceIdOutsideScope,
    DuplicateSourceId,
    ExtractionFailed,
    StorageFailed,
    EmbeddingPending,
    SidecarPending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetedRefreshItemResult {
    pub source_id: String,
    pub document_id: Option<String>,
    pub canonical: CanonicalRefreshStatus,
    pub embedding: DerivedRefreshStatus,
    pub sidecar: DerivedRefreshStatus,
    pub reasons: Vec<RefreshReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetedRefreshReport {
    pub preflight_passed: bool,
    pub reasons: Vec<RefreshReason>,
    pub items: Vec<TargetedRefreshItemResult>,
}

/// Executor bound to one canonical workspace and its SQLite store.
pub struct TargetedRefresher {
    config: Config,
    workspace: PathBuf,
    store: SqliteAppStore,
}

impl TargetedRefresher {
    pub async fn new(mut config: Config, workspace: &Path) -> Result<Self> {
        let workspace = workspace
            .canonicalize()
            .context("canonicalizing targeted-refresh workspace")?;
        if config.db.path.is_relative() {
            config.db.path = workspace.join(&config.db.path);
        }
        SqliteAppStore::initialize_config(&config).await?;
        let store = SqliteAppStore::connect(&config).await?;
        Ok(Self {
            config,
            workspace,
            store,
        })
    }

    pub async fn refresh(
        &self,
        source: &EnrolledSource,
        mut items: Vec<SourceItem>,
    ) -> TargetedRefreshReport {
        let mut batch_reasons = Vec::new();
        if items.is_empty() {
            batch_reasons.push(RefreshReason::EmptyBatch);
        }
        if items.len() > MAX_TARGETED_REFRESH_ITEMS {
            batch_reasons.push(RefreshReason::TooManyItems);
        }
        if source.workspace != self.workspace {
            batch_reasons.push(RefreshReason::WorkspaceMismatch);
        }
        if payload_bytes(&items).is_none_or(|bytes| bytes > MAX_TARGETED_REFRESH_PAYLOAD_BYTES) {
            batch_reasons.push(RefreshReason::PayloadTooLarge);
        }

        let duplicate_ids = duplicate_ids(&items);
        let mut item_reasons = Vec::with_capacity(items.len());
        for item in &mut items {
            let mut reasons = Vec::new();
            if item.source != source.source_label {
                reasons.push(RefreshReason::SourceMismatch);
            }
            if item.source_id.trim_ascii().is_empty() {
                reasons.push(RefreshReason::EmptySourceId);
            }
            if item.content_type.trim_ascii().is_empty() {
                reasons.push(RefreshReason::EmptyContentType);
            }
            if !matches!(
                serde_json::from_str::<serde_json::Value>(&item.metadata_json),
                Ok(serde_json::Value::Object(_))
            ) {
                reasons.push(RefreshReason::InvalidMetadata);
            }
            if item.raw_bytes.is_some() && !item.body.is_empty() {
                reasons.push(RefreshReason::RawBodyConflict);
            }
            if item
                .raw_bytes
                .as_ref()
                .is_some_and(|bytes| bytes.len() as u64 > source.max_extract_bytes)
            {
                reasons.push(RefreshReason::ItemTooLarge);
            }
            if !source.accepts(&item.source_id) {
                reasons.push(RefreshReason::SourceIdOutsideScope);
            }
            if duplicate_ids.contains(&item.source_id) {
                reasons.push(RefreshReason::DuplicateSourceId);
            }
            if reasons.is_empty() {
                if let Err(error) = prepare_source_item(item, source.max_extract_bytes) {
                    reasons.push(match error {
                        DocumentPreparationError::TooLarge { .. } => RefreshReason::ItemTooLarge,
                        DocumentPreparationError::Extraction(_) => RefreshReason::ExtractionFailed,
                    });
                }
            }
            item_reasons.push(reasons);
        }

        let rejected = !batch_reasons.is_empty() || item_reasons.iter().any(|r| !r.is_empty());
        if rejected {
            return TargetedRefreshReport {
                preflight_passed: false,
                reasons: batch_reasons,
                items: items
                    .into_iter()
                    .zip(item_reasons)
                    .map(|(item, reasons)| TargetedRefreshItemResult {
                        source_id: item.source_id,
                        document_id: None,
                        canonical: if reasons.is_empty() {
                            CanonicalRefreshStatus::NotAttempted
                        } else {
                            CanonicalRefreshStatus::Rejected
                        },
                        embedding: DerivedRefreshStatus::NotAttempted,
                        sidecar: DerivedRefreshStatus::NotAttempted,
                        reasons,
                    })
                    .collect(),
            };
        }

        let mut results = Vec::with_capacity(items.len());
        for item in items {
            match self
                .store
                .replace_source_item_atomic(&item, self.config.chunking.max_tokens)
                .await
            {
                Ok(write) => {
                    let (embedding, sidecar, reasons) = self
                        .refresh_derived(&write.chunks, write.sidecar_current)
                        .await;
                    results.push(TargetedRefreshItemResult {
                        source_id: item.source_id,
                        document_id: Some(write.document_id),
                        canonical: CanonicalRefreshStatus::Indexed,
                        embedding,
                        sidecar,
                        reasons,
                    });
                }
                Err(_) => results.push(TargetedRefreshItemResult {
                    source_id: item.source_id,
                    document_id: None,
                    canonical: CanonicalRefreshStatus::Failed,
                    embedding: DerivedRefreshStatus::NotAttempted,
                    sidecar: DerivedRefreshStatus::NotAttempted,
                    reasons: vec![RefreshReason::StorageFailed],
                }),
            }
        }
        TargetedRefreshReport {
            preflight_passed: true,
            reasons: Vec::new(),
            items: results,
        }
    }

    async fn refresh_derived(
        &self,
        chunks: &[Chunk],
        sidecar_current_after_reset: bool,
    ) -> (
        DerivedRefreshStatus,
        DerivedRefreshStatus,
        Vec<RefreshReason>,
    ) {
        let sidecar_configured =
            matches!(self.config.vector_index.backend.as_str(), "auto" | "zvec");
        if !self.config.embedding.is_enabled() {
            return (
                DerivedRefreshStatus::NotConfigured,
                if sidecar_configured && sidecar_current_after_reset {
                    DerivedRefreshStatus::Current
                } else if sidecar_configured {
                    DerivedRefreshStatus::Pending
                } else {
                    DerivedRefreshStatus::NotConfigured
                },
                if sidecar_configured && !sidecar_current_after_reset {
                    vec![RefreshReason::SidecarPending]
                } else {
                    Vec::new()
                },
            );
        }

        let provider = match embedding::create_provider(&self.config.embedding) {
            Ok(provider) => provider,
            Err(_) => {
                return (
                    DerivedRefreshStatus::Pending,
                    if sidecar_configured {
                        DerivedRefreshStatus::Pending
                    } else {
                        DerivedRefreshStatus::NotConfigured
                    },
                    vec![RefreshReason::EmbeddingPending],
                )
            }
        };
        let model = provider.model_name().to_owned();
        let mut embedding_pending = false;
        let mut sidecar_pending = !sidecar_current_after_reset;
        for batch in chunks.chunks(self.config.embedding.batch_size) {
            let texts: Vec<_> = batch.iter().map(|chunk| chunk.text.clone()).collect();
            let vectors =
                match embedding::embed_texts(provider.as_ref(), &self.config.embedding, &texts)
                    .await
                {
                    Ok(vectors) if vectors.len() == batch.len() => vectors,
                    _ => {
                        embedding_pending = true;
                        continue;
                    }
                };
            for (chunk, vector) in batch.iter().zip(vectors) {
                let hash = hash_text(&chunk.text);
                if self
                    .store
                    .upsert_embedding(
                        &chunk.id,
                        &chunk.document_id,
                        &vector,
                        &model,
                        provider.dims(),
                        &hash,
                    )
                    .await
                    .is_err()
                {
                    if self
                        .store
                        .get_embedding_hash(&chunk.id, &model)
                        .await
                        .ok()
                        .flatten()
                        .as_deref()
                        == Some(hash.as_str())
                    {
                        sidecar_pending = true;
                    } else {
                        embedding_pending = true;
                    }
                }
            }
        }
        let mut reasons = Vec::new();
        if embedding_pending {
            reasons.push(RefreshReason::EmbeddingPending);
        }
        if sidecar_pending {
            reasons.push(RefreshReason::SidecarPending);
        }
        (
            if embedding_pending {
                DerivedRefreshStatus::Pending
            } else {
                DerivedRefreshStatus::Current
            },
            if !sidecar_configured {
                DerivedRefreshStatus::NotConfigured
            } else if sidecar_pending {
                DerivedRefreshStatus::Pending
            } else {
                DerivedRefreshStatus::Current
            },
            reasons,
        )
    }
}

fn payload_bytes(items: &[SourceItem]) -> Option<usize> {
    items.iter().try_fold(0usize, |total, item| {
        let strings = item
            .source
            .len()
            .checked_add(item.source_id.len())?
            .checked_add(item.source_url.as_ref().map_or(0, String::len))?
            .checked_add(item.title.as_ref().map_or(0, String::len))?
            .checked_add(item.author.as_ref().map_or(0, String::len))?
            .checked_add(item.content_type.len())?
            .checked_add(item.body.len())?
            .checked_add(item.metadata_json.len())?
            .checked_add(item.raw_json.as_ref().map_or(0, String::len))?
            .checked_add(item.raw_bytes.as_ref().map_or(0, Vec::len))?;
        total.checked_add(strings)
    })
}

fn duplicate_ids(items: &[SourceItem]) -> HashSet<String> {
    let mut counts = HashMap::new();
    for item in items {
        *counts.entry(item.source_id.clone()).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(id, count)| (count > 1).then_some(id))
        .collect()
}

fn normalize_relative(value: &str) -> Option<String> {
    let path = Path::new(value);
    if path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}

fn path_excludes(configured: &[String]) -> Result<GlobSet> {
    let mut patterns = vec![
        "**/.git/**".into(),
        "**/target/**".into(),
        "**/node_modules/**".into(),
    ];
    patterns.extend_from_slice(configured);
    globset(&patterns)
}

fn s3_excludes(configured: &[String]) -> Result<GlobSet> {
    let mut patterns = vec!["**/.git/**".into(), "**/node_modules/**".into()];
    patterns.extend_from_slice(configured);
    globset(&patterns)
}
