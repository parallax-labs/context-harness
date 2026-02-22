//! Lua scripted connector runtime.
//!
//! Loads `.lua` connector scripts at runtime and executes them in a sandboxed
//! Lua 5.4 VM. Each script implements `connector.scan(config) → items[]`,
//! returning documents that flow into the standard ingestion pipeline.
//!
//! # Architecture
//!
//! The Lua VM runs on a blocking thread via [`tokio::task::spawn_blocking`]
//! to avoid blocking the async runtime. HTTP calls use `reqwest::blocking`,
//! and `sleep()` uses `std::thread::sleep`.
//!
//! # Host APIs
//!
//! Scripts have access to sandboxed host APIs provided by [`crate::lua_runtime`]:
//!
//! | Module | Functions |
//! |--------|-----------|
//! | `http` | `get`, `post`, `put` |
//! | `json` | `parse`, `encode` |
//! | `env` | `get` |
//! | `log` | `info`, `warn`, `error`, `debug` |
//! | `fs` | `read`, `list` (sandboxed to script directory) |
//! | `base64` | `encode`, `decode` |
//! | `crypto` | `sha256`, `hmac_sha256` |
//! | `sleep` | `sleep(seconds)` |
//!
//! # Configuration
//!
//! ```toml
//! [connectors.script.jira]
//! path = "connectors/jira.lua"
//! timeout = 600
//! url = "https://mycompany.atlassian.net"
//! api_token = "${JIRA_API_TOKEN}"
//! ```
//!
//! See `docs/LUA_CONNECTORS.md` for the full specification.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use mlua::prelude::*;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::{Config, ScriptConnectorConfig};
use crate::lua_runtime::{register_all_host_apis, toml_table_to_lua};
use crate::models::SourceItem;
use crate::traits::Connector;

// ═══════════════════════════════════════════════════════════════════════
// Connector trait implementation
// ═══════════════════════════════════════════════════════════════════════

/// A Lua scripted connector instance that implements the [`Connector`] trait.
///
/// Wraps the [`scan_script`] function, allowing Lua connectors to be used
/// through the unified trait-based dispatch alongside built-in connectors.
pub struct ScriptConnector {
    /// Instance name (e.g. `"jira"`).
    name: String,
    /// Configuration for this script connector instance.
    config: ScriptConnectorConfig,
}

impl ScriptConnector {
    /// Create a new script connector instance.
    pub fn new(name: String, config: ScriptConnectorConfig) -> Self {
        Self { name, config }
    }
}

#[async_trait]
impl Connector for ScriptConnector {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Execute Lua scripts to ingest custom data sources"
    }

    fn connector_type(&self) -> &str {
        "script"
    }

    async fn scan(&self) -> Result<Vec<SourceItem>> {
        scan_script(&self.name, &self.config).await
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════

/// Scan a Lua script connector and return the ingested items.
///
/// Spawns the Lua VM on a blocking thread to avoid blocking the async
/// runtime. The script's `connector.scan(config)` is called with the
/// TOML config section (minus `path` and `timeout`) converted to a Lua table.
pub async fn scan_script(
    name: &str,
    script_config: &ScriptConnectorConfig,
) -> Result<Vec<SourceItem>> {
    let path = script_config.path.clone();
    let extra = script_config.extra.clone();
    let name = name.to_string();
    let timeout = script_config.timeout;

    tokio::task::spawn_blocking(move || run_lua_scan(&path, &extra, &name, timeout))
        .await
        .context("Lua connector task panicked")?
}

/// Scaffold a new connector script from a template.
///
/// Creates `connectors/<name>.lua` with a commented template showing
/// the connector interface and available host APIs.
pub fn scaffold_connector(name: &str) -> Result<()> {
    let dir = Path::new("connectors");
    std::fs::create_dir_all(dir)?;

    let filename = format!("{}.lua", name);
    let path = dir.join(&filename);

    if path.exists() {
        bail!("Connector script already exists: {}", path.display());
    }

    let template = format!(
        r#"--[[
  Context Harness Connector: {name}

  Configuration (add to ctx.toml):

    [connectors.script.{name}]
    path = "connectors/{name}.lua"
    # url = "https://api.example.com"
    # api_token = "${{{name_upper}_API_TOKEN}}"

  Sync:
    ctx sync script:{name}

  Test:
    ctx connector test connectors/{name}.lua
]]

connector = {{
    name = "{name}",
    version = "0.1.0",
    description = "TODO: describe what this connector ingests",
}}

--- Scan the data source and return a list of items to ingest.
--- @param config table Configuration from ctx.toml
--- @return table Array of source item tables
function connector.scan(config)
    local items = {{}}

    -- Example: fetch from an API
    --
    -- local resp = http.get(config.url .. "/api/items", {{
    --     headers = {{ ["Authorization"] = "Bearer " .. config.api_token }},
    -- }})
    -- if not resp.ok then
    --     log.error("API error: " .. resp.status)
    --     return items
    -- end
    -- for _, item in ipairs(resp.json) do
    --     table.insert(items, {{
    --         source_id = tostring(item.id),
    --         title = item.title,
    --         body = item.content or "",
    --         source_url = config.url .. "/items/" .. item.id,
    --         updated_at = item.updated_at,
    --     }})
    -- end

    log.info("Fetched " .. #items .. " items")
    return items
end
"#,
        name = name,
        name_upper = name.to_uppercase().replace('-', "_"),
    );

    std::fs::write(&path, template)?;
    println!("Created connector: {}", path.display());
    println!();
    println!("Add to your ctx.toml:");
    println!();
    println!("  [connectors.script.{}]", name);
    println!("  path = \"connectors/{}.lua\"", name);
    println!();
    println!("Then sync:");
    println!();
    println!("  ctx sync script:{}", name);

    Ok(())
}

/// Test a connector script without writing to the database.
///
/// Loads and executes the script, prints the returned items, and reports
/// any errors. Useful for development and debugging.
pub async fn test_script(path: &Path, config: &Config, source: Option<&str>) -> Result<()> {
    let script_path = path.to_path_buf();

    let extra = if let Some(name) = source {
        config
            .connectors
            .script
            .get(name)
            .map(|sc| sc.extra.clone())
            .unwrap_or_default()
    } else {
        toml::Table::new()
    };

    let name = source.unwrap_or("test").to_string();

    println!("Testing connector: {} ({})", name, script_path.display());

    let items = {
        let p = script_path.clone();
        let e = extra;
        let n = name.clone();
        tokio::task::spawn_blocking(move || run_lua_scan(&p, &e, &n, 300))
            .await
            .context("Lua connector task panicked")??
    };

    println!("  ✓ Script loaded and executed");
    println!("  ✓ Returned {} items", items.len());

    let valid = items
        .iter()
        .filter(|i| !i.body.is_empty() && !i.source_id.is_empty())
        .count();
    println!("  ✓ {} valid items", valid);

    if items.is_empty() {
        println!("  (no items returned)");
        return Ok(());
    }

    println!();
    let show = items.len().min(5);
    println!("Items (first {}):", show);
    for (i, item) in items.iter().take(show).enumerate() {
        println!(
            "  [{}] {}: {} ({})",
            i,
            item.source_id,
            item.title.as_deref().unwrap_or("untitled"),
            item.updated_at.format("%Y-%m-%d")
        );
    }
    if items.len() > show {
        println!("  ... and {} more", items.len() - show);
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Lua VM Execution (blocking)
// ═══════════════════════════════════════════════════════════════════════

/// Execute a Lua connector script and return the resulting source items.
///
/// This function runs synchronously on a blocking thread. It:
/// 1. Creates a sandboxed Lua VM via [`crate::lua_runtime`]
/// 2. Loads and executes the script
/// 3. Calls `connector.scan(config)`
/// 4. Converts the returned Lua table to `Vec<SourceItem>`
fn run_lua_scan(
    script_path: &Path,
    extra: &toml::Table,
    name: &str,
    timeout_secs: u64,
) -> Result<Vec<SourceItem>> {
    let script_src = std::fs::read_to_string(script_path)
        .with_context(|| format!("Failed to read connector script: {}", script_path.display()))?;

    let script_dir = script_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let lua = Lua::new();

    // Set up timeout via instruction hook
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(10_000),
        move |_lua, _debug| {
            if Instant::now() > deadline {
                Err(mlua::Error::RuntimeError(format!(
                    "script timed out after {} seconds",
                    timeout_secs
                )))
            } else {
                Ok(mlua::VmState::Continue)
            }
        },
    );

    // Register all shared host APIs
    let log_name = format!("script:{}", name);
    register_all_host_apis(&lua, &log_name, &script_dir)?;

    // Load and execute the script
    lua.load(&script_src)
        .set_name(script_path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute connector script {}: {}",
                script_path.display(),
                e
            )
        })?;

    // Build the config table (with env var expansion)
    let config_table = toml_table_to_lua(&lua, extra)?;

    // Call connector.scan(config)
    let connector: LuaTable = lua
        .globals()
        .get::<LuaTable>("connector")
        .map_err(|e| anyhow::anyhow!("Script must define a global 'connector' table: {}", e))?;

    let scan: LuaFunction = connector
        .get::<LuaFunction>("scan")
        .map_err(|e| anyhow::anyhow!("connector.scan function not defined: {}", e))?;

    let result: LuaTable = scan.call::<LuaTable>(config_table).map_err(|e| {
        anyhow::anyhow!(
            "connector.scan() failed in '{}': {}",
            script_path.display(),
            e
        )
    })?;

    // Convert Lua result to Vec<SourceItem>
    lua_table_to_source_items(result, name)
}

// ═══════════════════════════════════════════════════════════════════════
// Value Conversions: Lua → SourceItem
// ═══════════════════════════════════════════════════════════════════════

/// Convert a Lua array table into a Vec of SourceItems.
///
/// Invalid items (missing required fields, empty body) are logged as
/// warnings and skipped — they do not cause the sync to fail.
fn lua_table_to_source_items(table: LuaTable, connector_name: &str) -> Result<Vec<SourceItem>> {
    let mut items = Vec::new();
    let default_source = format!("script:{}", connector_name);

    for pair in table.pairs::<i64, LuaTable>() {
        let (idx, item_table) =
            pair.map_err(|e| anyhow::anyhow!("Invalid item in scan result: {}", e))?;

        // Required: source_id
        let source_id: String = match item_table.get::<String>("source_id") {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "[script:{}] WARN: Skipping item at index {}: missing 'source_id'",
                    connector_name, idx
                );
                continue;
            }
        };

        // Required: body
        let body: String = match item_table.get::<String>("body") {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "[script:{}] WARN: Skipping item '{}': missing 'body'",
                    connector_name, source_id
                );
                continue;
            }
        };

        if body.is_empty() {
            eprintln!(
                "[script:{}] WARN: Skipping item '{}': empty body",
                connector_name, source_id
            );
            continue;
        }

        // Optional fields
        let source: String = item_table
            .get::<String>("source")
            .unwrap_or_else(|_| default_source.clone());
        let title: Option<String> = item_table.get::<String>("title").ok();
        let author: Option<String> = item_table.get::<String>("author").ok();
        let source_url: Option<String> = item_table.get::<String>("source_url").ok();
        let content_type: String = item_table
            .get::<String>("content_type")
            .unwrap_or_else(|_| "text/plain".to_string());
        let metadata_json: String = item_table
            .get::<String>("metadata_json")
            .unwrap_or_else(|_| "{}".to_string());

        // Timestamps
        let now = Utc::now();
        let updated_at = parse_lua_timestamp(&item_table, "updated_at").unwrap_or(now);
        let created_at = parse_lua_timestamp(&item_table, "created_at").unwrap_or(updated_at);

        items.push(SourceItem {
            source,
            source_id,
            source_url,
            title,
            author,
            created_at,
            updated_at,
            content_type,
            body,
            metadata_json,
            raw_json: None,
        });
    }

    Ok(items)
}

/// Parse a timestamp from a Lua table field.
///
/// Supports ISO 8601 strings (with or without timezone) and Unix timestamps
/// (integer or float). Returns `None` if the field is missing or unparseable.
fn parse_lua_timestamp(table: &LuaTable, field: &str) -> Option<DateTime<Utc>> {
    // Try as string (ISO 8601 / RFC 3339)
    if let Ok(s) = table.get::<String>(field) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S") {
            return Some(dt.and_utc());
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S") {
            return Some(dt.and_utc());
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%SZ") {
            return Some(dt.and_utc());
        }
    }

    // Try as number (Unix timestamp)
    if let Ok(ts) = table.get::<i64>(field) {
        return Utc.timestamp_opt(ts, 0).single();
    }
    if let Ok(ts) = table.get::<f64>(field) {
        return Utc.timestamp_opt(ts as i64, 0).single();
    }

    None
}
