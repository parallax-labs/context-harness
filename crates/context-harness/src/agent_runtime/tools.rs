//! Read-only runtime adapters using the existing Tool/ToolRegistry and core
//! retrieval implementation. No connector processes, embeddings or sidecars.
use crate::{
    config::Config,
    sqlite_store::SqliteStore,
    traits::{GetTool, SearchTool, Tool, ToolContext, ToolRegistry},
};
use anyhow::{ensure, Context, Result};
use async_trait::async_trait;
use context_harness_core::{
    search::{search, SearchParams, SearchRequest},
    store::Store,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Filters {
    source: Option<String>,
    since: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    query: String,
    #[serde(default = "keyword")]
    mode: String,
    #[serde(default = "limit")]
    limit: i64,
    filters: Option<Filters>,
}
fn keyword() -> String {
    "keyword".into()
}
fn limit() -> i64 {
    12
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    id: String,
}

pub(super) fn validate(name: &str, arguments: &Value) -> Result<()> {
    match name {
        "search" => {
            let args: SearchArgs = serde_json::from_value(arguments.clone())?;
            ensure!(
                !args.query.trim().is_empty() && args.query.len() <= 8192,
                "invalid query"
            );
            ensure!(
                args.mode == "keyword",
                "runtime search supports keyword mode only"
            );
            ensure!(
                (1..=100).contains(&args.limit),
                "limit must be between 1 and 100"
            );
            if let Some(filters) = args.filters {
                if let Some(source) = filters.source {
                    ensure!(source.len() <= 1024, "source is too long");
                }
                if let Some(since) = filters.since {
                    let date = chrono::NaiveDate::parse_from_str(&since, "%Y-%m-%d")?;
                    ensure!(date.format("%Y-%m-%d").to_string() == since, "invalid date");
                }
            }
        }
        "get" => {
            let args: GetArgs = serde_json::from_value(arguments.clone())?;
            ensure!(
                !args.id.trim().is_empty() && args.id.len() <= 128 && !args.id.contains(':'),
                "invalid document id"
            );
        }
        _ => anyhow::bail!("unsupported runtime tool"),
    }
    Ok(())
}

pub(super) async fn read_pool(config: &Config) -> Result<SqlitePool> {
    Ok(SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&config.db.path)
                .read_only(true)
                .create_if_missing(false)
                .pragma("query_only", "ON"),
        )
        .await?)
}

pub(super) async fn registry(config: &Config) -> Result<ToolRegistry> {
    let pool = read_pool(config).await?;
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ReadSearch {
        pool: pool.clone(),
        candidate_limit: config.retrieval.candidate_k_keyword.clamp(1, 1000),
    }));
    registry.register(Box::new(ReadGet { pool }));
    Ok(registry)
}

struct ReadSearch {
    pool: SqlitePool,
    candidate_limit: i64,
}
#[async_trait]
impl Tool for ReadSearch {
    fn capabilities(&self) -> Option<Vec<crate::agent_resource::Capability>> {
        Some(vec![crate::agent_resource::Capability::ReadOnly])
    }
    fn name(&self) -> &str {
        "search"
    }
    fn description(&self) -> &str {
        "Search this workspace's indexed context using local keywords"
    }
    fn is_builtin(&self) -> bool {
        true
    }
    fn parameters_schema(&self) -> Value {
        let mut schema = SearchTool.parameters_schema();
        schema["additionalProperties"] = json!(false);
        schema["properties"]["mode"]["enum"] = json!(["keyword"]);
        schema["properties"]["limit"]["minimum"] = json!(1);
        schema["properties"]["limit"]["maximum"] = json!(100);
        schema["properties"]["filters"]["additionalProperties"] = json!(false);
        schema
    }
    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        validate(self.name(), arguments)
    }
    async fn execute(&self, arguments: Value, _ctx: &ToolContext) -> Result<Value> {
        validate("search", &arguments)?;
        let args: SearchArgs = serde_json::from_value(arguments)?;
        let filters = args.filters.as_ref();
        let request = SearchRequest {
            query: &args.query,
            query_vec: None,
            mode: "keyword",
            source_filter: filters.and_then(|f| f.source.as_deref()),
            since: filters.and_then(|f| f.since.as_deref()),
            explain: false,
            params: SearchParams {
                hybrid_alpha: 0.0,
                candidate_k_keyword: self.candidate_limit,
                candidate_k_vector: 0,
                final_limit: args.limit,
            },
        };
        Ok(json!({"results": search(&SqliteStore::new(self.pool.clone()), &request).await?}))
    }
}
struct ReadGet {
    pool: SqlitePool,
}
#[async_trait]
impl Tool for ReadGet {
    fn capabilities(&self) -> Option<Vec<crate::agent_resource::Capability>> {
        Some(vec![crate::agent_resource::Capability::ReadOnly])
    }
    fn name(&self) -> &str {
        "get"
    }
    fn description(&self) -> &str {
        "Retrieve an indexed document from this workspace"
    }
    fn is_builtin(&self) -> bool {
        true
    }
    fn parameters_schema(&self) -> Value {
        let mut schema = GetTool.parameters_schema();
        schema["additionalProperties"] = json!(false);
        schema
    }
    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        validate(self.name(), arguments)
    }
    async fn execute(&self, arguments: Value, _ctx: &ToolContext) -> Result<Value> {
        validate("get", &arguments)?;
        let args: GetArgs = serde_json::from_value(arguments)?;
        let document = SqliteStore::new(self.pool.clone())
            .get_document(&args.id)
            .await?
            .context("document not found")?;
        Ok(serde_json::to_value(document)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_or_malformed_arguments() {
        for args in [
            json!({"query":"x", "mode":"semantic"}),
            json!({"query":"x", "limit":-1}),
            json!({"query":"x", "limit":"1"}),
            json!({"query":"x", "workspace":"other"}),
            json!({"query":"x", "filters":{"since":"2026-02-30"}}),
        ] {
            assert!(validate("search", &args).is_err());
        }
        assert!(validate("get", &json!({"id":"other:doc"})).is_err());
        assert!(validate("get", &json!({"id":"doc", "workspace":"other"})).is_err());
    }
    #[tokio::test]
    async fn retrieval_cannot_create_a_missing_database() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = Config::minimal();
        config.db.path = tmp.path().join("absent.sqlite");
        assert!(registry(&config).await.is_err());
        assert!(!config.db.path.exists());
    }
}
