//! Export the search index as JSON for static site search.
//!
//! Produces a `data.json` file containing all documents and chunks,
//! suitable for use with `ctx-search.js` on static sites. Replaces
//! the Python one-liner previously used in `build-docs.sh`.

use anyhow::Result;
use std::path::Path;

use crate::app_store::{AppStore, SqliteAppStore};
use crate::config::Config;

/// Export documents and chunks as JSON.
///
/// If `output` is `Some`, writes to that file path. Otherwise writes
/// to stdout for piping.
pub async fn run_export(config: &Config, output: Option<&Path>) -> Result<()> {
    let store = SqliteAppStore::connect(config).await?;
    let data = store.export_index().await?;
    let doc_count = data.documents.len();
    let chunk_count = data.chunks.len();
    let json = serde_json::to_string_pretty(&data)?;

    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &json)?;
            eprintln!(
                "Exported {} documents, {} chunks to {}",
                doc_count,
                chunk_count,
                path.display()
            );
        }
        None => {
            println!("{}", json);
        }
    }

    store.close().await;
    Ok(())
}
