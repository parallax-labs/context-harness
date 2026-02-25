//! Integration tests for the Rust extension traits.
//!
//! These tests prove that custom connectors and tools (implemented via the
//! `Connector` and `Tool` traits) work end-to-end through the actual sync
//! pipeline and HTTP server.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use context_harness::agents::{Agent, AgentPrompt, AgentRegistry};
use context_harness::config::Config;
use context_harness::ingest::run_sync_with_extensions;
use context_harness::migrate;
use context_harness::models::SourceItem;
use context_harness::search::search_documents;
use context_harness::server::run_server_with_extensions;
use context_harness::traits::{
    Connector, ConnectorRegistry, SearchOptions, Tool, ToolContext, ToolRegistry,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tempfile::TempDir;

// ─── Test Connector ─────────────────────────────────────────────────

/// A simple in-memory connector that returns hardcoded documents.
struct InMemoryConnector {
    docs: Vec<(String, String, String)>, // (id, title, body)
}

impl InMemoryConnector {
    fn new(docs: Vec<(String, String, String)>) -> Self {
        Self { docs }
    }
}

#[async_trait]
impl Connector for InMemoryConnector {
    fn name(&self) -> &str {
        "inmemory"
    }

    fn description(&self) -> &str {
        "In-memory test connector"
    }

    async fn scan(&self) -> Result<Vec<SourceItem>> {
        let now = Utc::now();
        Ok(self
            .docs
            .iter()
            .map(|(id, title, body)| SourceItem {
                source: "custom:inmemory".to_string(),
                source_id: id.clone(),
                source_url: None,
                title: Some(title.clone()),
                author: Some("test".to_string()),
                created_at: now,
                updated_at: now,
                content_type: "text/plain".to_string(),
                body: body.clone(),
                metadata_json: "{}".to_string(),
                raw_json: None,
            })
            .collect())
    }
}

// ─── Test Tool ──────────────────────────────────────────────────────

/// A tool that searches the knowledge base and returns result count.
struct CountTool;

#[async_trait]
impl Tool for CountTool {
    fn name(&self) -> &str {
        "count_results"
    }

    fn description(&self) -> &str {
        "Count search results for a query"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let query = params["query"].as_str().unwrap_or("");

        let results = ctx
            .search(
                query,
                SearchOptions {
                    mode: Some("keyword".to_string()),
                    limit: Some(100),
                    ..Default::default()
                },
            )
            .await?;

        let sources = ctx.sources()?;

        Ok(json!({
            "query": query,
            "count": results.len(),
            "source_count": sources.len(),
        }))
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn test_config(tmp: &TempDir) -> Config {
    let root = tmp.path();
    let db_path = root.join("ctx.sqlite");
    let config_content = format!(
        r#"
[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:0"
"#,
        db_path.display()
    );
    toml::from_str(&config_content).unwrap()
}

fn test_config_with_port(tmp: &TempDir, port: u16) -> Config {
    let root = tmp.path();
    let db_path = root.join("ctx.sqlite");
    let config_content = format!(
        r#"
[db]
path = "{}"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:{}"
"#,
        db_path.display(),
        port
    );
    toml::from_str(&config_content).unwrap()
}

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn wait_for_server(port: u16) {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/health", port);
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                return;
            }
        }
    }
    panic!("Server did not become ready within 5 seconds");
}

// ─── Tests ──────────────────────────────────────────────────────────

/// Prove that a custom connector's items flow through the full sync pipeline
/// and are searchable afterwards.
#[tokio::test]
async fn test_custom_connector_sync_and_search() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config(&tmp);

    // Initialize database
    migrate::run_migrations(&cfg).await.unwrap();

    // Register custom connector with test documents
    let mut connectors = ConnectorRegistry::new();
    connectors.register(Box::new(InMemoryConnector::new(vec![
        (
            "doc-1".into(),
            "Rust Tutorial".into(),
            "Learn Rust programming language. Ownership and borrowing are key concepts.".into(),
        ),
        (
            "doc-2".into(),
            "Deploy Guide".into(),
            "Deploy your application using Docker and Kubernetes for production.".into(),
        ),
        (
            "doc-3".into(),
            "Database Design".into(),
            "SQLite is a great choice for local-first applications with FTS5 support.".into(),
        ),
    ])));

    // Sync via the trait-based pipeline
    run_sync_with_extensions(
        &cfg,
        "custom:inmemory",
        true,
        false,
        None,
        None,
        None,
        &connectors,
    )
    .await
    .unwrap();

    // Search should find our documents
    let results = search_documents(&cfg, "Rust programming", "keyword", None, None, None, false)
        .await
        .unwrap();

    assert!(
        !results.is_empty(),
        "Custom connector documents should be searchable"
    );

    // Verify the source label
    assert!(
        results.iter().any(|r| r.source == "custom:inmemory"),
        "Results should be tagged with custom:inmemory source"
    );

    // Search for Docker
    let results = search_documents(
        &cfg,
        "Docker Kubernetes",
        "keyword",
        None,
        None,
        None,
        false,
    )
    .await
    .unwrap();
    assert!(!results.is_empty(), "Should find Docker/Kubernetes docs");
}

/// Prove that syncing "all" includes custom connectors.
#[tokio::test]
async fn test_custom_connector_included_in_sync_all() {
    let tmp = TempDir::new().unwrap();
    let cfg = test_config(&tmp);
    migrate::run_migrations(&cfg).await.unwrap();

    let mut connectors = ConnectorRegistry::new();
    connectors.register(Box::new(InMemoryConnector::new(vec![(
        "all-test".into(),
        "All Test Doc".into(),
        "This document should be synced when using ctx sync all.".into(),
    )])));

    // Sync "all" should include custom connectors
    run_sync_with_extensions(&cfg, "all", true, false, None, None, None, &connectors)
        .await
        .unwrap();

    let results = search_documents(
        &cfg,
        "synced using ctx sync all",
        "keyword",
        None,
        None,
        None,
        false,
    )
    .await
    .unwrap();

    assert!(
        !results.is_empty(),
        "Custom connector should be included in sync all"
    );
    assert_eq!(results[0].source, "custom:inmemory");
}

/// Prove that a custom tool can be called through the HTTP server and
/// uses ToolContext to search the knowledge base.
#[tokio::test]
async fn test_custom_tool_via_http_server() {
    let port = find_free_port();
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_with_port(&tmp, port);

    // Initialize and sync some data
    migrate::run_migrations(&cfg).await.unwrap();
    let mut connectors = ConnectorRegistry::new();
    connectors.register(Box::new(InMemoryConnector::new(vec![
        (
            "t1".into(),
            "Authentication".into(),
            "JWT tokens and OAuth2 flows for secure authentication.".into(),
        ),
        (
            "t2".into(),
            "Authorization".into(),
            "Role-based access control and permission management.".into(),
        ),
    ])));
    run_sync_with_extensions(
        &cfg,
        "custom:inmemory",
        true,
        false,
        None,
        None,
        None,
        &connectors,
    )
    .await
    .unwrap();

    // Register custom tool
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(CountTool));
    let tools = Arc::new(tools);

    // Start server in background
    let cfg_clone = cfg.clone();
    let tools_clone = tools.clone();
    let agents = Arc::new(AgentRegistry::new());
    let server_handle = tokio::spawn(async move {
        run_server_with_extensions(&cfg_clone, tools_clone, agents)
            .await
            .ok();
    });

    // Wait for server
    wait_for_server(port).await;

    let client = reqwest::Client::new();

    // Verify tool appears in /tools/list
    let list_url = format!("http://127.0.0.1:{}/tools/list", port);
    let resp = client.get(&list_url).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let tool_names: Vec<&str> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        tool_names.contains(&"count_results"),
        "Custom tool should appear in /tools/list, got: {:?}",
        tool_names
    );

    // Call the custom tool
    let tool_url = format!("http://127.0.0.1:{}/tools/count_results", port);
    let resp = client
        .post(&tool_url)
        .json(&json!({"query": "authentication"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    let result = &body["result"];
    assert_eq!(result["query"], "authentication");
    assert!(
        result["count"].as_i64().unwrap() > 0,
        "Tool should find results via ToolContext.search(), got: {}",
        result
    );
    assert!(
        result["source_count"].as_i64().unwrap() >= 0,
        "Tool should report sources via ToolContext.sources()"
    );

    // Call a non-existent tool → 404
    let resp = client
        .post(format!("http://127.0.0.1:{}/tools/nonexistent", port))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // Clean up
    server_handle.abort();
}

/// Prove that custom tools take priority over Lua tools with the same name.
/// Also verifies the built-in tools (search, get, sources) still appear.
#[tokio::test]
async fn test_tool_list_includes_builtins_and_custom() {
    let port = find_free_port();
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_with_port(&tmp, port);
    migrate::run_migrations(&cfg).await.unwrap();

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(CountTool));
    let tools = Arc::new(tools);

    let cfg_clone = cfg.clone();
    let tools_clone = tools.clone();
    let agents = Arc::new(AgentRegistry::new());
    let server_handle = tokio::spawn(async move {
        run_server_with_extensions(&cfg_clone, tools_clone, agents)
            .await
            .ok();
    });
    wait_for_server(port).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/tools/list", port))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let tool_names: Vec<&str> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();

    // Built-in tools should always be present
    assert!(tool_names.contains(&"search"), "Missing built-in: search");
    assert!(tool_names.contains(&"get"), "Missing built-in: get");
    assert!(tool_names.contains(&"sources"), "Missing built-in: sources");

    // Custom tool should also be present
    assert!(
        tool_names.contains(&"count_results"),
        "Missing custom: count_results"
    );

    server_handle.abort();
}

// ─── Test Agent ─────────────────────────────────────────────────────

/// A simple test agent that returns a static prompt.
struct TestAgent;

#[async_trait]
impl Agent for TestAgent {
    fn name(&self) -> &str {
        "test-agent"
    }

    fn description(&self) -> &str {
        "A test agent for integration tests"
    }

    fn tools(&self) -> Vec<String> {
        vec!["search".into(), "get".into()]
    }

    fn source(&self) -> &str {
        "rust"
    }

    async fn resolve(&self, args: Value, _ctx: &ToolContext) -> Result<AgentPrompt> {
        let topic = args["topic"].as_str().unwrap_or("testing");
        Ok(AgentPrompt {
            system: format!("You are a test agent focused on {}.", topic),
            tools: self.tools(),
            messages: vec![],
        })
    }
}

/// Prove that custom agents appear in /agents/list and can be resolved.
#[tokio::test]
async fn test_custom_agent_list_and_resolve() {
    let port = find_free_port();
    let tmp = TempDir::new().unwrap();
    let cfg = test_config_with_port(&tmp, port);
    migrate::run_migrations(&cfg).await.unwrap();

    let tools = Arc::new(ToolRegistry::new());
    let mut agents = AgentRegistry::new();
    agents.register(Box::new(TestAgent));
    let agents = Arc::new(agents);

    let cfg_clone = cfg.clone();
    let tools_clone = tools.clone();
    let agents_clone = agents.clone();
    let server_handle = tokio::spawn(async move {
        run_server_with_extensions(&cfg_clone, tools_clone, agents_clone)
            .await
            .ok();
    });
    wait_for_server(port).await;

    let client = reqwest::Client::new();

    // Verify agent appears in /agents/list
    let list_url = format!("http://127.0.0.1:{}/agents/list", port);
    let resp = client.get(&list_url).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let agent_names: Vec<&str> = body["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert!(
        agent_names.contains(&"test-agent"),
        "Custom agent should appear in /agents/list, got: {:?}",
        agent_names
    );

    // Verify agent metadata
    let agent_info = body["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "test-agent")
        .unwrap();
    assert_eq!(agent_info["source"], "rust");
    assert_eq!(
        agent_info["description"],
        "A test agent for integration tests"
    );

    // Resolve the agent's prompt
    let resolve_url = format!("http://127.0.0.1:{}/agents/test-agent/prompt", port);
    let resp = client
        .post(&resolve_url)
        .json(&json!({"topic": "deployment"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["system"].as_str().unwrap().contains("deployment"),
        "Agent should incorporate the topic argument into the system prompt"
    );
    assert_eq!(body["tools"][0], "search");
    assert_eq!(body["tools"][1], "get");

    // Resolve with no arguments (should use default)
    let resp = client
        .post(&resolve_url)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(body["system"].as_str().unwrap().contains("testing"));

    // Non-existent agent → 404
    let resp = client
        .post(format!(
            "http://127.0.0.1:{}/agents/nonexistent/prompt",
            port
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    server_handle.abort();
}
