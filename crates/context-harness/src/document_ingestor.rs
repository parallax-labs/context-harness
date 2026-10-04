//! Shared per-item document ingestion used by connector-driven full sync.
//!
//! Discovery, filtering, progress and checkpoints remain owned by `ingest`.

use crate::app_store::AppStore;
use crate::chunk::chunk_text;
use crate::config::Config;
use crate::embed_cmd;
use crate::extract;
use crate::models::SourceItem;
use anyhow::Result;

const DEFAULT_MAX_EXTRACT_BYTES: u64 = 50_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IngestedDocument {
    pub(crate) chunks_written: u64,
    pub(crate) embeddings_written: u64,
    pub(crate) embeddings_pending: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentIngestOutcome {
    Ingested(IngestedDocument),
    ExtractionSkipped,
}

pub(crate) enum DocumentPreparationError {
    TooLarge { size: usize, limit: u64 },
    Extraction(extract::ExtractError),
}

pub(crate) fn prepare_source_item(
    item: &mut SourceItem,
    max_extract_bytes: u64,
) -> std::result::Result<(), DocumentPreparationError> {
    let Some(bytes) = item.raw_bytes.as_ref() else {
        return Ok(());
    };
    if bytes.len() as u64 > max_extract_bytes {
        return Err(DocumentPreparationError::TooLarge {
            size: bytes.len(),
            limit: max_extract_bytes,
        });
    }
    let body = extract::extract_text(bytes, &item.content_type)
        .map_err(DocumentPreparationError::Extraction)?;
    item.body = body;
    item.raw_bytes = None;
    Ok(())
}

/// Executes the existing canonical write path for one already-discovered item.
pub(crate) struct DocumentIngestor<'a, S> {
    config: &'a Config,
    store: &'a S,
    max_extract_bytes: u64,
}

impl<'a, S: AppStore> DocumentIngestor<'a, S> {
    pub(crate) fn new(config: &'a Config, store: &'a S, source_label: &str) -> Self {
        Self {
            config,
            store,
            max_extract_bytes: max_extract_bytes_for_source(config, source_label),
        }
    }

    pub(crate) async fn ingest(&self, item: &mut SourceItem) -> Result<DocumentIngestOutcome> {
        if let Err(error) = prepare_source_item(item, self.max_extract_bytes) {
            match error {
                DocumentPreparationError::TooLarge { size, limit } => eprintln!(
                    "Warning: skipping {} (size {} > max_extract_bytes {})",
                    item.source_id, size, limit
                ),
                DocumentPreparationError::Extraction(error) => {
                    eprintln!(
                        "Warning: extraction failed for {}: {}",
                        item.source_id, error
                    );
                }
            }
            return Ok(DocumentIngestOutcome::ExtractionSkipped);
        }

        let document_id = self.store.upsert_source_item(item).await?;
        let chunks = chunk_text(&document_id, &item.body, self.config.chunking.max_tokens);
        let chunks_written = chunks.len() as u64;
        self.store
            .replace_chunks(&document_id, &chunks, None)
            .await?;
        let (embeddings_written, embeddings_pending) =
            embed_cmd::embed_chunks_inline(self.config, self.store, &chunks).await;

        Ok(DocumentIngestOutcome::Ingested(IngestedDocument {
            chunks_written,
            embeddings_written,
            embeddings_pending,
        }))
    }
}

fn max_extract_bytes_for_source(config: &Config, source_label: &str) -> u64 {
    if let Some(name) = source_label.strip_prefix("filesystem:") {
        config
            .connectors
            .filesystem
            .get(name)
            .map(|connector| connector.max_extract_bytes)
            .unwrap_or(DEFAULT_MAX_EXTRACT_BYTES)
    } else {
        DEFAULT_MAX_EXTRACT_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_store::SqliteAppStore;
    use chrono::{TimeZone, Utc};
    use sqlx::Row;
    use tempfile::TempDir;

    fn config(temp: &TempDir) -> Config {
        toml::from_str(&format!(
            r#"
[db]
path = "{}"

[chunking]
max_tokens = 3

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:0"
"#,
            temp.path().join("ctx.sqlite").display()
        ))
        .unwrap()
    }

    fn item(body: &str) -> SourceItem {
        SourceItem {
            source: "custom:fixture".into(),
            source_id: "doc-1".into(),
            source_url: None,
            title: Some("Fixture".into()),
            author: None,
            created_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            updated_at: Utc.timestamp_opt(1_700_000_001, 0).unwrap(),
            content_type: "text/plain".into(),
            body: body.into(),
            metadata_json: "{}".into(),
            raw_json: None,
            raw_bytes: None,
        }
    }

    #[tokio::test]
    async fn replaces_canonical_chunks_without_touching_discovery_checkpoint() {
        let temp = TempDir::new().unwrap();
        let config = config(&temp);
        SqliteAppStore::initialize_config(&config).await.unwrap();
        let store = SqliteAppStore::connect(&config).await.unwrap();
        let ingestor = DocumentIngestor::new(&config, &store, "custom:fixture");

        let mut original = item("obsolete alpha paragraph\n\nsecond paragraph");
        let first = ingestor.ingest(&mut original).await.unwrap();
        assert!(matches!(first, DocumentIngestOutcome::Ingested(_)));

        let mut replacement = item("replacement beta text");
        let second = ingestor.ingest(&mut replacement).await.unwrap();
        let DocumentIngestOutcome::Ingested(second) = second else {
            panic!("text input should be ingested");
        };
        assert_eq!(second.embeddings_written, 0);
        assert_eq!(second.embeddings_pending, 0);
        assert_eq!(store.get_checkpoint("custom:fixture").await.unwrap(), None);

        let rows = sqlx::query(
            "SELECT c.text FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.source = ? AND d.source_id = ? ORDER BY c.chunk_index",
        )
        .bind("custom:fixture")
        .bind("doc-1")
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert_eq!(rows.len() as u64, second.chunks_written);
        let text = rows
            .iter()
            .map(|row| row.get::<String, _>("text"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("replacement beta text"));
        assert!(!text.contains("obsolete alpha"));
    }
}
