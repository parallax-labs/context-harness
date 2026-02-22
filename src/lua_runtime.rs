//! Shared Lua 5.4 VM runtime for connectors and tools.
//!
//! Provides a sandboxed Lua environment with host APIs that both
//! [`crate::connector_script`] and [`crate::tool_script`] use. The Lua VM
//! runs on a blocking thread (via [`tokio::task::spawn_blocking`]), so all
//! host functions use synchronous I/O (`reqwest::blocking`, `std::thread::sleep`).
//!
//! # Host APIs
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
//! # Sandboxing
//!
//! Dangerous Lua standard libraries (`os`, `io`, `debug`, `loadfile`, `dofile`)
//! are removed. Filesystem access is restricted to a configurable sandbox root
//! directory.

use globset::Glob;
use hmac::{Hmac, Mac};
use mlua::prelude::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

// ═══════════════════════════════════════════════════════════════════════
// Public helpers
// ═══════════════════════════════════════════════════════════════════════

/// Register all standard host APIs on a Lua VM instance.
///
/// This is the single entry-point used by both connector and tool runtimes.
/// It sandboxes the globals and registers every host module.
///
/// # Arguments
///
/// * `lua` — the Lua VM instance to configure.
/// * `script_name` — logical name used for log prefixes (e.g. `"script:jira"`).
/// * `sandbox_root` — directory that `fs.read` / `fs.list` are confined to.
pub(crate) fn register_all_host_apis(
    lua: &Lua,
    script_name: &str,
    sandbox_root: &Path,
) -> LuaResult<()> {
    sandbox_globals(lua)?;
    register_http_api(lua)?;
    register_json_api(lua)?;
    register_env_api(lua)?;
    register_log_api(lua, script_name)?;
    register_fs_api(lua, sandbox_root)?;
    register_base64_api(lua)?;
    register_crypto_api(lua)?;
    register_sleep(lua)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Sandboxing
// ═══════════════════════════════════════════════════════════════════════

/// Remove dangerous standard library functions from the Lua globals.
pub(crate) fn sandbox_globals(lua: &Lua) -> LuaResult<()> {
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

fn register_log_api(lua: &Lua, script_name: &str) -> LuaResult<()> {
    let log = lua.create_table()?;

    let n = script_name.to_string();
    log.set(
        "info",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[{}] INFO: {}", n, msg);
            Ok(())
        })?,
    )?;

    let n = script_name.to_string();
    log.set(
        "warn",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[{}] WARN: {}", n, msg);
            Ok(())
        })?,
    )?;

    let n = script_name.to_string();
    log.set(
        "error",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[{}] ERROR: {}", n, msg);
            Ok(())
        })?,
    )?;

    let n = script_name.to_string();
    log.set(
        "debug",
        lua.create_function(move |_lua, msg: String| {
            eprintln!("[{}] DEBUG: {}", n, msg);
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
pub(crate) fn toml_value_to_lua(lua: &Lua, value: &toml::Value) -> LuaResult<LuaValue> {
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
pub(crate) fn toml_table_to_lua(lua: &Lua, table: &toml::Table) -> LuaResult<LuaTable> {
    let lua_table = lua.create_table()?;
    for (k, v) in table {
        lua_table.set(k.as_str(), toml_value_to_lua(lua, v)?)?;
    }
    Ok(lua_table)
}

/// Expand `${VAR_NAME}` patterns in a string from the process environment.
pub(crate) fn expand_env_vars(s: &str) -> String {
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
pub(crate) fn json_value_to_lua(lua: &Lua, value: &serde_json::Value) -> LuaResult<LuaValue> {
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
pub(crate) fn lua_value_to_json(value: LuaValue) -> LuaResult<serde_json::Value> {
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
