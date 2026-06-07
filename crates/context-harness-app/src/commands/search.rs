use serde::Serialize;
use tauri::State;

use crate::state::AppState;

use super::AppError;

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f64,
    pub title: Option<String>,
    pub source: String,
    pub source_id: String,
    pub updated_at: String,
    pub snippet: String,
    pub source_url: Option<String>,
}

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    mode: String,
    limit: Option<i64>,
    source: Option<String>,
    since: Option<String>,
    explain: Option<bool>,
) -> Result<Vec<SearchResult>, AppError> {
    let ws = state.workspace.read().await;
    let workspace = ws.as_ref().ok_or(AppError::NoWorkspace)?;

    let results = context_harness::search::search_documents(
        &workspace.config,
        &query,
        &mode,
        source.as_deref(),
        since.as_deref(),
        limit,
        explain.unwrap_or(false),
    )
    .await
    .map_err(|e| AppError::SearchError(e.to_string()))?;

    Ok(results
        .into_iter()
        .map(|r| SearchResult {
            id: r.id,
            score: r.score,
            title: r.title,
            source: r.source,
            source_id: r.source_id,
            updated_at: r.updated_at,
            snippet: r.snippet,
            source_url: r.source_url,
        })
        .collect())
}
