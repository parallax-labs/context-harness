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
//! Scripts have access to sandboxed host APIs:
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
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use globset::Glob;
use hmac::{Hmac, Mac};
use mlua::prelude::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::{Config, ScriptConnectorConfig};
use crate::models::SourceItem;

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
/// 1. Creates a sandboxed Lua VM
/// 2. Registers all host APIs
/// 3. Loads and executes the script
/// 4. Calls `connector.scan(config)`
/// 5. Converts the returned Lua table to `Vec<SourceItem>`
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

    // Sandbox: remove dangerous globals
    sandbox_globals(&lua)?;

    // Register host APIs
    register_http_api(&lua)?;
    register_json_api(&lua)?;
    register_env_api(&lua)?;
    register_log_api(&lua, name)?;
    register_fs_api(&lua, &script_dir)?;
    register_base64_api(&lua)?;
    register_crypto_api(&lua)?;
    register_sleep(&lua)?;

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
// Sandboxing
// ═══════════════════════════════════════════════════════════════════════

/// Remove dangerous standard library functions from the Lua globals.
fn sandbox_globals(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();
    globals.set("os", LuaValue::Nil)?;
    globals.set("io", LuaValue::Nil)?;
    globals.set("loadfile", LuaValue::Nil)?;
    globals.set("dofile", LuaValue::Nil)?;
    globals.set("debug", LuaValue::Nil)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: http
// ═══════════════════════════════════════════════════════════════════════

fn register_http_api(lua: &Lua) -> LuaResult<()> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(mlua::Error::external)?;

    let http = lua.create_table()?;

    // http.get(url, opts?) → response
    let c = client.clone();
    http.set(
        "get",
        lua.create_function(move |lua, (url, opts): (String, Option<LuaTable>)| {
            do_http_request(lua, &c, "GET", &url, None, opts)
        })?,
    )?;

    // http.post(url, body, opts?) → response
    let c = client.clone();
    http.set(
        "post",
        lua.create_function(
            move |lua, (url, body, opts): (String, String, Option<LuaTable>)| {
                do_http_request(lua, &c, "POST", &url, Some(&body), opts)
            },
        )?,
    )?;

    // http.put(url, body, opts?) → response
    let c = client.clone();
    http.set(
        "put",
        lua.create_function(
            move |lua, (url, body, opts): (String, String, Option<LuaTable>)| {
                do_http_request(lua, &c, "PUT", &url, Some(&body), opts)
            },
        )?,
    )?;

    lua.globals().set("http", http)?;
    Ok(())
}

/// Execute an HTTP request and return a Lua table with the response.
fn do_http_request(
    lua: &Lua,
    client: &reqwest::blocking::Client,
    method: &str,
    url: &str,
    body: Option<&str>,
    opts: Option<LuaTable>,
) -> LuaResult<LuaTable> {
    let mut builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        _ => {
            return Err(mlua::Error::external(anyhow::anyhow!(
                "unsupported HTTP method: {}",
                method
            )))
        }
    };

    if let Some(ref opts) = opts {
        // Headers
        if let Ok(headers) = opts.get::<LuaTable>("headers") {
            for pair in headers.pairs::<String, String>() {
                let (k, v) = pair?;
                builder = builder.header(k, v);
            }
        }

        // Query parameters
        if let Ok(params) = opts.get::<LuaTable>("params") {
            let mut param_vec: Vec<(String, String)> = Vec::new();
            for pair in params.pairs::<String, String>() {
                let (k, v) = pair?;
                param_vec.push((k, v));
            }
            builder = builder.query(&param_vec);
        }

        // Custom timeout
        if let Ok(timeout) = opts.get::<f64>("timeout") {
            builder = builder.timeout(Duration::from_secs_f64(timeout));
        }
    }

    // Request body
    if let Some(body) = body {
        builder = builder.body(body.to_string());
    }

    // Execute the request
    let response = builder.send().map_err(|e| {
        mlua::Error::external(anyhow::anyhow!("HTTP {} {} failed: {}", method, url, e))
    })?;

    let status = response.status().as_u16();
    let ok = response.status().is_success();

    // Collect response headers
    let headers_table = lua.create_table()?;
    for (name, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            headers_table.set(name.as_str(), v.to_string())?;
        }
    }

    // Read body
    let body_text = response.text().map_err(|e| {
        mlua::Error::external(anyhow::anyhow!("Failed to read response body: {}", e))
    })?;

    // Try to parse as JSON
    let json_value = serde_json::from_str::<serde_json::Value>(&body_text).ok();

    // Build result table
    let result = lua.create_table()?;
    result.set("status", status)?;
    result.set("headers", headers_table)?;
    result.set("body", body_text)?;
    result.set("ok", ok)?;
    if let Some(json) = json_value {
        result.set("json", json_value_to_lua(lua, &json)?)?;
    }

    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: json
// ═══════════════════════════════════════════════════════════════════════

fn register_json_api(lua: &Lua) -> LuaResult<()> {
    let json_table = lua.create_table()?;

    json_table.set(
        "parse",
        lua.create_function(|lua, s: String| {
            let value: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("json.parse: {}", e)))?;
            json_value_to_lua(lua, &value)
        })?,
    )?;

    json_table.set(
        "encode",
        lua.create_function(|_lua, value: LuaValue| {
            let json = lua_value_to_json(value)?;
            serde_json::to_string(&json)
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("json.encode: {}", e)))
        })?,
    )?;

    lua.globals().set("json", json_table)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: env
// ═══════════════════════════════════════════════════════════════════════

fn register_env_api(lua: &Lua) -> LuaResult<()> {
    let env = lua.create_table()?;

    env.set(
        "get",
        lua.create_function(|_lua, name: String| Ok(std::env::var(&name).ok()))?,
    )?;

    lua.globals().set("env", env)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: log
// ═══════════════════════════════════════════════════════════════════════

fn register_log_api(lua: &Lua, connector_name: &str) -> LuaResult<()> {
    let log = lua.create_table()?;

    let n = connector_name.to_string();
    log.set(
        "info",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[script:{}] INFO: {}", n, msg);
            Ok(())
        })?,
    )?;

    let n = connector_name.to_string();
    log.set(
        "warn",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[script:{}] WARN: {}", n, msg);
            Ok(())
        })?,
    )?;

    let n = connector_name.to_string();
    log.set(
        "error",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[script:{}] ERROR: {}", n, msg);
            Ok(())
        })?,
    )?;

    let n = connector_name.to_string();
    log.set(
        "debug",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[script:{}] DEBUG: {}", n, msg);
            Ok(())
        })?,
    )?;

    lua.globals().set("log", log)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: fs (sandboxed)
// ═══════════════════════════════════════════════════════════════════════

fn register_fs_api(lua: &Lua, sandbox_root: &Path) -> LuaResult<()> {
    let fs = lua.create_table()?;
    let root = sandbox_root
        .canonicalize()
        .unwrap_or_else(|_| sandbox_root.to_path_buf());

    // fs.read(path) → string
    let r = root.clone();
    fs.set(
        "read",
        lua.create_function(move |_lua, path: String| {
            let target = r.join(&path);
            let canonical = target
                .canonicalize()
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("fs.read: {}: {}", path, e)))?;
            if !canonical.starts_with(&r) {
                return Err(mlua::Error::external(anyhow::anyhow!(
                    "fs.read: path escapes sandbox: {}",
                    path
                )));
            }
            std::fs::read_to_string(&canonical)
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("fs.read: {}: {}", path, e)))
        })?,
    )?;

    // fs.list(dir, glob?) → [{path, size, modified}]
    let r = root.clone();
    fs.set(
        "list",
        lua.create_function(move |lua, (dir, glob_pattern): (String, Option<String>)| {
            let target = r.join(&dir);
            let canonical = target
                .canonicalize()
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("fs.list: {}: {}", dir, e)))?;
            if !canonical.starts_with(&r) {
                return Err(mlua::Error::external(anyhow::anyhow!(
                    "fs.list: path escapes sandbox: {}",
                    dir
                )));
            }

            let matcher = glob_pattern
                .as_deref()
                .map(|p| {
                    Glob::new(p)
                        .map_err(|e| {
                            mlua::Error::external(anyhow::anyhow!("fs.list: bad glob: {}", e))
                        })
                        .map(|g| g.compile_matcher())
                })
                .transpose()?;

            let entries = lua.create_table()?;
            let mut idx = 1i64;

            let dir_entries = std::fs::read_dir(&canonical)
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("fs.list: {}: {}", dir, e)))?;

            for entry in dir_entries {
                let entry = entry.map_err(mlua::Error::external)?;
                let path = entry.path();

                if let Some(ref m) = matcher {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !m.is_match(name) {
                        continue;
                    }
                }

                let metadata = entry.metadata().map_err(mlua::Error::external)?;
                let item = lua.create_table()?;
                item.set("path", path.to_string_lossy().to_string())?;
                item.set("size", metadata.len())?;
                item.set(
                    "modified",
                    metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                )?;
                entries.set(idx, item)?;
                idx += 1;
            }

            Ok(entries)
        })?,
    )?;

    lua.globals().set("fs", fs)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: base64
// ═══════════════════════════════════════════════════════════════════════

fn register_base64_api(lua: &Lua) -> LuaResult<()> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let b64 = lua.create_table()?;

    b64.set(
        "encode",
        lua.create_function(|_lua, data: String| Ok(STANDARD.encode(data.as_bytes())))?,
    )?;

    b64.set(
        "decode",
        lua.create_function(|_lua, data: String| {
            let bytes = STANDARD
                .decode(data.as_bytes())
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("base64.decode: {}", e)))?;
            String::from_utf8(bytes)
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("base64.decode: {}", e)))
        })?,
    )?;

    lua.globals().set("base64", b64)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: crypto
// ═══════════════════════════════════════════════════════════════════════

fn register_crypto_api(lua: &Lua) -> LuaResult<()> {
    let crypto = lua.create_table()?;

    crypto.set(
        "sha256",
        lua.create_function(|_lua, data: String| {
            let mut hasher = Sha256::new();
            hasher.update(data.as_bytes());
            Ok(format!("{:x}", hasher.finalize()))
        })?,
    )?;

    crypto.set(
        "hmac_sha256",
        lua.create_function(|_lua, (key, data): (String, String)| {
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(key.as_bytes())
                .map_err(|e| mlua::Error::external(anyhow::anyhow!("crypto.hmac_sha256: {}", e)))?;
            mac.update(data.as_bytes());
            Ok(hex::encode(mac.finalize().into_bytes()))
        })?,
    )?;

    lua.globals().set("crypto", crypto)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Host API: sleep
// ═══════════════════════════════════════════════════════════════════════

fn register_sleep(lua: &Lua) -> LuaResult<()> {
    lua.globals().set(
        "sleep",
        lua.create_function(|_lua, seconds: f64| {
            std::thread::sleep(Duration::from_secs_f64(seconds));
            Ok(())
        })?,
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Value Conversions: TOML → Lua
// ═══════════════════════════════════════════════════════════════════════

/// Convert a TOML value to a Lua value, expanding `${VAR}` in strings.
fn toml_value_to_lua(lua: &Lua, value: &toml::Value) -> LuaResult<LuaValue> {
    match value {
        toml::Value::String(s) => {
            let expanded = expand_env_vars(s);
            lua.create_string(&expanded).map(LuaValue::String)
        }
        toml::Value::Integer(i) => Ok(LuaValue::Integer(*i)),
        toml::Value::Float(f) => Ok(LuaValue::Number(*f)),
        toml::Value::Boolean(b) => Ok(LuaValue::Boolean(*b)),
        toml::Value::Array(arr) => {
            let table = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                table.set(i as i64 + 1, toml_value_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(table))
        }
        toml::Value::Table(map) => {
            let t = toml_table_to_lua(lua, map)?;
            Ok(LuaValue::Table(t))
        }
        _ => Ok(LuaValue::Nil),
    }
}

/// Convert a TOML table to a Lua table.
fn toml_table_to_lua(lua: &Lua, table: &toml::Table) -> LuaResult<LuaTable> {
    let lua_table = lua.create_table()?;
    for (k, v) in table {
        lua_table.set(k.as_str(), toml_value_to_lua(lua, v)?)?;
    }
    Ok(lua_table)
}

/// Expand `${VAR_NAME}` patterns in a string from the process environment.
fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find("${") {
        let end = match result[start..].find('}') {
            Some(pos) => start + pos,
            None => break,
        };
        let var_name = &result[start + 2..end];
        let value = std::env::var(var_name).unwrap_or_default();
        result = format!("{}{}{}", &result[..start], value, &result[end + 1..]);
    }
    result
}

// ═══════════════════════════════════════════════════════════════════════
// Value Conversions: JSON ↔ Lua
// ═══════════════════════════════════════════════════════════════════════

/// Convert a JSON value to a Lua value.
fn json_value_to_lua(lua: &Lua, value: &serde_json::Value) -> LuaResult<LuaValue> {
    match value {
        serde_json::Value::Null => Ok(LuaValue::Nil),
        serde_json::Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(LuaValue::Integer(i))
            } else {
                Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0)))
            }
        }
        serde_json::Value::String(s) => lua.create_string(s).map(LuaValue::String),
        serde_json::Value::Array(arr) => {
            let table = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                table.set(i as i64 + 1, json_value_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(table))
        }
        serde_json::Value::Object(map) => {
            let table = lua.create_table()?;
            for (k, v) in map {
                table.set(k.as_str(), json_value_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(table))
        }
    }
}

/// Convert a Lua value to a JSON value.
fn lua_value_to_json(value: LuaValue) -> LuaResult<serde_json::Value> {
    match value {
        LuaValue::Nil => Ok(serde_json::Value::Null),
        LuaValue::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        LuaValue::Integer(i) => Ok(serde_json::Value::Number(i.into())),
        LuaValue::Number(n) => Ok(serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)),
        LuaValue::String(s) => Ok(serde_json::Value::String(s.to_str()?.to_string())),
        LuaValue::Table(t) => {
            // Heuristic: if raw_len > 0, treat as array; otherwise as object
            let len = t.raw_len();
            if len > 0 {
                let mut arr = Vec::new();
                for i in 1..=len {
                    let v: LuaValue = t.raw_get(i)?;
                    arr.push(lua_value_to_json(v)?);
                }
                Ok(serde_json::Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.pairs::<String, LuaValue>() {
                    let (k, v) = pair?;
                    map.insert(k, lua_value_to_json(v)?);
                }
                Ok(serde_json::Value::Object(map))
            }
        }
        _ => Ok(serde_json::Value::Null),
    }
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
