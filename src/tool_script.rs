//! Lua MCP tool extensions.
//!
//! Loads `.lua` tool scripts at server startup, extracts their parameter schemas,
//! and provides execution with a context bridge back into the Rust core
//! (search, get, sources).
//!
//! # Architecture
//!
//! Tool scripts follow the same sandboxed Lua VM model as connectors
//! ([`crate::connector_script`]), reusing all host APIs from
//! [`crate::lua_runtime`]. In addition, tools receive a `context` table with:
//!
//! - `context.search(query, opts?)` — search the knowledge base
//! - `context.get(id)` — retrieve a document by UUID
//! - `context.sources()` — list connector status
//! - `context.config` — tool-specific configuration from `ctx.toml`
//!
//! # Script Interface
//!
//! Every tool script defines a global `tool` table:
//!
//! ```lua
//! tool = {
//!     name = "my_tool",
//!     description = "Does something useful",
//!     parameters = {
//!         { name = "query", type = "string", required = true, description = "Search query" },
//!     },
//! }
//!
//! function tool.execute(params, context)
//!     local results = context.search(params.query)
//!     return { results = results }
//! end
//! ```
//!
//! # Configuration
//!
//! ```toml
//! [tools.script.my_tool]
//! path = "tools/my-tool.lua"
//! timeout = 30
//! api_key = "${MY_API_KEY}"
//! ```
//!
//! See `docs/LUA_TOOLS.md` for the full specification.

use anyhow::{bail, Context, Result};
use mlua::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::{Config, ScriptToolConfig};
use crate::get::{get_document, DocumentResponse};
use crate::lua_runtime::{
    json_value_to_lua, lua_value_to_json, register_all_host_apis, toml_table_to_lua,
};
use crate::search::{search_documents, SearchResultItem};
use crate::sources::{get_sources, SourceStatus};

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

/// Metadata extracted from a loaded Lua tool script.
///
/// Created at server startup by [`load_tool_definitions`]. Contains
/// everything needed to register the tool with the HTTP server and
/// execute it when called.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Tool identifier (matches the config key in `[tools.script.<name>]`).
    pub name: String,
    /// One-line description for agent discovery.
    pub description: String,
    /// OpenAI function-calling JSON Schema for the tool's parameters.
    pub parameters_schema: serde_json::Value,
    /// Path to the `.lua` script file.
    pub script_path: PathBuf,
    /// Raw Lua source code (cached to avoid re-reading on every call).
    pub script_source: String,
    /// Tool-specific config keys from `ctx.toml` (passed as `context.config`).
    pub config: toml::Table,
    /// Maximum execution time in seconds.
    pub timeout: u64,
}

/// Serializable tool info for the `/tools/list` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// Whether this is a built-in tool (`true`) or a Lua tool (`false`).
    pub builtin: bool,
    /// OpenAI function-calling JSON Schema.
    pub parameters: serde_json::Value,
}

// ═══════════════════════════════════════════════════════════════════════
// Loading
// ═══════════════════════════════════════════════════════════════════════

/// Load all tool scripts from config and extract their definitions.
///
/// For each `[tools.script.<name>]` entry, reads the script file, creates
/// a temporary Lua VM to extract the `tool` table metadata, and converts
/// the parameter declarations to OpenAI JSON Schema.
///
/// Called once at server startup.
pub fn load_tool_definitions(config: &Config) -> Result<Vec<ToolDefinition>> {
    let mut tools = Vec::new();

    for (name, tool_config) in &config.tools.script {
        let tool_def = load_single_tool(name, tool_config)
            .with_context(|| format!("Failed to load tool script '{}'", name))?;
        tools.push(tool_def);
    }

    Ok(tools)
}

/// Load a single tool script and extract its definition.
fn load_single_tool(name: &str, tool_config: &ScriptToolConfig) -> Result<ToolDefinition> {
    let script_src = std::fs::read_to_string(&tool_config.path)
        .with_context(|| format!("Failed to read tool script: {}", tool_config.path.display()))?;

    // Create a temporary Lua VM just to extract metadata
    let lua = Lua::new();
    lua.load(&script_src)
        .set_name(tool_config.path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute tool script {}: {}",
                tool_config.path.display(),
                e
            )
        })?;

    let tool_table: LuaTable = lua
        .globals()
        .get::<LuaTable>("tool")
        .map_err(|e| anyhow::anyhow!("Script must define a global 'tool' table: {}", e))?;

    let description: String = tool_table
        .get::<String>("description")
        .unwrap_or_else(|_| format!("Lua tool: {}", name));

    let params_table: LuaTable = tool_table
        .get::<LuaTable>("parameters")
        .unwrap_or_else(|_| lua.create_table().expect("create_table"));

    let schema = lua_params_to_json_schema(&params_table)?;

    Ok(ToolDefinition {
        name: name.to_string(),
        description,
        parameters_schema: schema,
        script_path: tool_config.path.clone(),
        script_source: script_src,
        config: tool_config.extra.clone(),
        timeout: tool_config.timeout,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// Schema Conversion
// ═══════════════════════════════════════════════════════════════════════

/// Convert Lua parameter declarations to OpenAI function-calling JSON Schema.
///
/// Input format (Lua array of tables):
/// ```lua
/// {
///     { name = "title", type = "string", required = true, description = "Ticket title" },
///     { name = "priority", type = "string", enum = { "low", "medium", "high" } },
/// }
/// ```
///
/// Output format:
/// ```json
/// {
///     "type": "object",
///     "properties": {
///         "title": { "type": "string", "description": "Ticket title" },
///         "priority": { "type": "string", "enum": ["low", "medium", "high"] }
///     },
///     "required": ["title"]
/// }
/// ```
fn lua_params_to_json_schema(params: &LuaTable) -> Result<serde_json::Value> {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    let len = params.raw_len();
    for i in 1..=len {
        let param: LuaTable = params
            .raw_get(i)
            .map_err(|e| anyhow::anyhow!("Invalid parameter at index {}: {}", i, e))?;

        let param_name: String = param
            .get::<String>("name")
            .map_err(|e| anyhow::anyhow!("Parameter at index {} missing 'name': {}", i, e))?;

        let param_type: String = param
            .get::<String>("type")
            .unwrap_or_else(|_| "string".to_string());

        let mut prop = serde_json::Map::new();
        prop.insert("type".to_string(), serde_json::json!(param_type));

        if let Ok(desc) = param.get::<String>("description") {
            prop.insert("description".to_string(), serde_json::json!(desc));
        }

        if let Ok(default_val) = param.get::<LuaValue>("default") {
            if !matches!(default_val, LuaValue::Nil) {
                let json_default = lua_value_to_json(default_val)
                    .map_err(|e| anyhow::anyhow!("Invalid default for '{}': {}", param_name, e))?;
                prop.insert("default".to_string(), json_default);
            }
        }

        if let Ok(enum_table) = param.get::<LuaTable>("enum") {
            let mut enum_values = Vec::new();
            let enum_len = enum_table.raw_len();
            for j in 1..=enum_len {
                let val: LuaValue = enum_table.raw_get(j)?;
                let json_val = lua_value_to_json(val).map_err(|e| {
                    anyhow::anyhow!("Invalid enum value for '{}': {}", param_name, e)
                })?;
                enum_values.push(json_val);
            }
            prop.insert("enum".to_string(), serde_json::Value::Array(enum_values));
        }

        let is_required = param.get::<bool>("required").unwrap_or(false);
        if is_required {
            required.push(serde_json::json!(param_name));
        }

        properties.insert(param_name, serde_json::Value::Object(prop));
    }

    let mut schema = serde_json::Map::new();
    schema.insert("type".to_string(), serde_json::json!("object"));
    schema.insert(
        "properties".to_string(),
        serde_json::Value::Object(properties),
    );
    if !required.is_empty() {
        schema.insert("required".to_string(), serde_json::Value::Array(required));
    }

    Ok(serde_json::Value::Object(schema))
}

// ═══════════════════════════════════════════════════════════════════════
// Parameter Validation
// ═══════════════════════════════════════════════════════════════════════

/// Validate incoming JSON parameters against a tool's schema.
///
/// Checks required fields, type compatibility, and enum constraints.
/// Injects default values for missing optional fields. Returns the
/// validated (and potentially enriched) parameters.
pub fn validate_params(
    schema: &serde_json::Value,
    params: &serde_json::Value,
) -> Result<serde_json::Value> {
    let params_obj = params
        .as_object()
        .unwrap_or(&serde_json::Map::new())
        .clone();

    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default();

    let required: Vec<String> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut result = params_obj.clone();

    // Check required fields
    for req_field in &required {
        if !params_obj.contains_key(req_field) {
            bail!("missing required parameter: {}", req_field);
        }
    }

    // Type checking and enum validation
    for (prop_name, prop_schema) in &properties {
        if let Some(value) = params_obj.get(prop_name) {
            // Type check
            if let Some(expected_type) = prop_schema.get("type").and_then(|t| t.as_str()) {
                let type_ok = match expected_type {
                    "string" => value.is_string(),
                    "integer" => value.is_i64() || value.is_u64(),
                    "number" => value.is_number(),
                    "boolean" => value.is_boolean(),
                    "array" => value.is_array(),
                    "object" => value.is_object(),
                    _ => true,
                };
                if !type_ok {
                    bail!(
                        "parameter '{}' must be of type '{}', got {}",
                        prop_name,
                        expected_type,
                        json_type_name(value)
                    );
                }
            }

            // Enum validation
            if let Some(enum_values) = prop_schema.get("enum").and_then(|e| e.as_array()) {
                if !enum_values.contains(value) {
                    let allowed: Vec<String> = enum_values.iter().map(|v| v.to_string()).collect();
                    bail!(
                        "parameter '{}' must be one of [{}], got {}",
                        prop_name,
                        allowed.join(", "),
                        value
                    );
                }
            }
        } else {
            // Inject default value if available
            if let Some(default) = prop_schema.get("default") {
                result.insert(prop_name.clone(), default.clone());
            }
        }
    }

    Ok(serde_json::Value::Object(result))
}

/// Return a human-readable name for a JSON value's type.
fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Execution
// ═══════════════════════════════════════════════════════════════════════

/// Execute a tool script with the given parameters.
///
/// Spawns a blocking thread, creates a sandboxed Lua VM with all host APIs
/// plus the context bridge, and calls `tool.execute(params, context)`.
///
/// # Arguments
///
/// * `tool` — tool definition (script source, config, timeout).
/// * `params` — validated JSON parameters for the tool.
/// * `app_config` — full application config (needed for context bridge).
///
/// # Returns
///
/// The JSON value returned by `tool.execute()`.
pub async fn execute_tool(
    tool: &ToolDefinition,
    params: serde_json::Value,
    app_config: &Config,
) -> Result<serde_json::Value> {
    let tool = tool.clone();
    let config = app_config.clone();

    tokio::task::spawn_blocking(move || run_lua_tool(&tool, params, &config))
        .await
        .context("Lua tool task panicked")?
}

/// Run the Lua tool synchronously on a blocking thread.
fn run_lua_tool(
    tool: &ToolDefinition,
    params: serde_json::Value,
    config: &Config,
) -> Result<serde_json::Value> {
    let script_dir = tool
        .script_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let lua = Lua::new();

    // Set up timeout via instruction hook
    let timeout_secs = tool.timeout;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(10_000),
        move |_lua, _debug| {
            if Instant::now() > deadline {
                Err(mlua::Error::RuntimeError(format!(
                    "tool timed out after {} seconds",
                    timeout_secs
                )))
            } else {
                Ok(mlua::VmState::Continue)
            }
        },
    );

    // Register all shared host APIs
    let log_name = format!("tool:{}", tool.name);
    register_all_host_apis(&lua, &log_name, &script_dir)?;

    // Register context bridge
    register_context_bridge(&lua, config, &tool.config)?;

    // Load and execute the script
    lua.load(&tool.script_source)
        .set_name(tool.script_path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute tool script {}: {}",
                tool.script_path.display(),
                e
            )
        })?;

    // Get tool.execute function
    let tool_table: LuaTable = lua
        .globals()
        .get::<LuaTable>("tool")
        .map_err(|e| anyhow::anyhow!("Script must define a global 'tool' table: {}", e))?;

    let execute: LuaFunction = tool_table
        .get::<LuaFunction>("execute")
        .map_err(|e| anyhow::anyhow!("tool.execute function not defined: {}", e))?;

    // Convert params to Lua table
    let params_lua = json_value_to_lua(&lua, &params)?;

    // Get the context table we already registered
    let context: LuaTable = lua
        .globals()
        .get::<LuaTable>("context")
        .map_err(|e| anyhow::anyhow!("context table missing: {}", e))?;

    // Call tool.execute(params, context)
    let result: LuaValue = execute
        .call::<LuaValue>((params_lua, context))
        .map_err(|e| {
            anyhow::anyhow!(
                "tool.execute() failed in '{}': {}",
                tool.script_path.display(),
                e
            )
        })?;

    // Convert result to JSON
    lua_value_to_json(result)
        .map_err(|e| anyhow::anyhow!("Failed to convert tool result to JSON: {}", e))
}

// ═══════════════════════════════════════════════════════════════════════
// Context Bridge
// ═══════════════════════════════════════════════════════════════════════

/// Register the `context` table in the Lua VM.
///
/// Provides `context.search`, `context.get`, `context.sources`, and
/// `context.config`. The first three call back into Rust's async core
/// via `tokio::runtime::Handle::block_on`.
fn register_context_bridge(lua: &Lua, config: &Config, tool_config: &toml::Table) -> LuaResult<()> {
    let ctx = lua.create_table()?;

    // context.config — tool-specific config from ctx.toml
    let config_lua = toml_table_to_lua(lua, tool_config)?;
    ctx.set("config", config_lua)?;

    // context.search(query, opts?) → results
    let cfg = config.clone();
    ctx.set(
        "search",
        lua.create_function(move |lua, (query, opts): (String, Option<LuaTable>)| {
            let mode = opts
                .as_ref()
                .and_then(|o| o.get::<String>("mode").ok())
                .unwrap_or_else(|| "keyword".to_string());
            let limit = opts
                .as_ref()
                .and_then(|o| o.get::<i64>("limit").ok())
                .unwrap_or(12);
            let source = opts.as_ref().and_then(|o| o.get::<String>("source").ok());

            let handle = tokio::runtime::Handle::current();
            let results = handle
                .block_on(async {
                    search_documents(&cfg, &query, &mode, source.as_deref(), None, Some(limit))
                        .await
                })
                .map_err(mlua::Error::external)?;

            search_results_to_lua(lua, &results)
        })?,
    )?;

    // context.get(id) → document
    let cfg = config.clone();
    ctx.set(
        "get",
        lua.create_function(move |lua, id: String| {
            let handle = tokio::runtime::Handle::current();
            let doc = handle
                .block_on(async { get_document(&cfg, &id).await })
                .map_err(mlua::Error::external)?;

            doc_response_to_lua(lua, &doc)
        })?,
    )?;

    // context.sources() → sources
    let cfg = config.clone();
    ctx.set(
        "sources",
        lua.create_function(move |lua, ()| {
            let sources = get_sources(&cfg);
            sources_to_lua(lua, &sources)
        })?,
    )?;

    lua.globals().set("context", ctx)?;
    Ok(())
}

/// Convert search results to a Lua array table.
fn search_results_to_lua(lua: &Lua, results: &[SearchResultItem]) -> LuaResult<LuaTable> {
    let table = lua.create_table()?;
    for (i, item) in results.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("id", item.id.as_str())?;
        row.set("score", item.score)?;
        row.set("source", item.source.as_str())?;
        row.set("source_id", item.source_id.as_str())?;
        row.set("updated_at", item.updated_at.as_str())?;
        row.set("snippet", item.snippet.as_str())?;
        if let Some(ref title) = item.title {
            row.set("title", title.as_str())?;
        }
        if let Some(ref url) = item.source_url {
            row.set("source_url", url.as_str())?;
        }
        table.set(i as i64 + 1, row)?;
    }
    Ok(table)
}

/// Convert a document response to a Lua table.
fn doc_response_to_lua(lua: &Lua, doc: &DocumentResponse) -> LuaResult<LuaTable> {
    let table = lua.create_table()?;
    table.set("id", doc.id.as_str())?;
    table.set("source", doc.source.as_str())?;
    table.set("source_id", doc.source_id.as_str())?;
    table.set("content_type", doc.content_type.as_str())?;
    table.set("body", doc.body.as_str())?;
    table.set("created_at", doc.created_at.as_str())?;
    table.set("updated_at", doc.updated_at.as_str())?;
    if let Some(ref title) = doc.title {
        table.set("title", title.as_str())?;
    }
    if let Some(ref author) = doc.author {
        table.set("author", author.as_str())?;
    }
    if let Some(ref url) = doc.source_url {
        table.set("source_url", url.as_str())?;
    }

    // Chunks
    let chunks_table = lua.create_table()?;
    for (i, chunk) in doc.chunks.iter().enumerate() {
        let c = lua.create_table()?;
        c.set("index", chunk.index)?;
        c.set("text", chunk.text.as_str())?;
        chunks_table.set(i as i64 + 1, c)?;
    }
    table.set("chunks", chunks_table)?;

    Ok(table)
}

/// Convert source statuses to a Lua array table.
fn sources_to_lua(lua: &Lua, sources: &[SourceStatus]) -> LuaResult<LuaTable> {
    let table = lua.create_table()?;
    for (i, s) in sources.iter().enumerate() {
        let row = lua.create_table()?;
        row.set("name", s.name.as_str())?;
        row.set("configured", s.configured)?;
        row.set("healthy", s.healthy)?;
        if let Some(ref notes) = s.notes {
            row.set("notes", notes.as_str())?;
        }
        table.set(i as i64 + 1, row)?;
    }
    Ok(table)
}

// ═══════════════════════════════════════════════════════════════════════
// Tool Discovery
// ═══════════════════════════════════════════════════════════════════════

/// Build the list of all tools (built-in + Lua) for the `/tools/list` endpoint.
pub fn build_tool_list(lua_tools: &[ToolDefinition]) -> Vec<ToolInfo> {
    let mut tools = Vec::new();

    // Built-in tools
    tools.push(ToolInfo {
        name: "search".to_string(),
        description: "Search the knowledge base".to_string(),
        builtin: true,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"], "default": "keyword" },
                "limit": { "type": "integer", "description": "Max results", "default": 12 },
                "filters": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Filter by connector source" },
                        "since": { "type": "string", "description": "Only results updated after this date (YYYY-MM-DD)" }
                    }
                }
            },
            "required": ["query"]
        }),
    });

    tools.push(ToolInfo {
        name: "get".to_string(),
        description: "Retrieve a document by UUID".to_string(),
        builtin: true,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "Document UUID" }
            },
            "required": ["id"]
        }),
    });

    tools.push(ToolInfo {
        name: "sources".to_string(),
        description: "List connector configuration and health status".to_string(),
        builtin: true,
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    });

    // Lua tools
    for tool in lua_tools {
        tools.push(ToolInfo {
            name: tool.name.clone(),
            description: tool.description.clone(),
            builtin: false,
            parameters: tool.parameters_schema.clone(),
        });
    }

    tools
}

// ═══════════════════════════════════════════════════════════════════════
// CLI: scaffold & test
// ═══════════════════════════════════════════════════════════════════════

/// Scaffold a new tool script from a template.
///
/// Creates `tools/<name>.lua` with a commented template showing the
/// tool interface and available host APIs.
pub fn scaffold_tool(name: &str) -> Result<()> {
    let dir = Path::new("tools");
    std::fs::create_dir_all(dir)?;

    let filename = format!("{}.lua", name.replace('_', "-"));
    let path = dir.join(&filename);

    if path.exists() {
        bail!("Tool script already exists: {}", path.display());
    }

    let template = format!(
        r#"--[[
  Context Harness Tool: {name}

  Configuration (add to ctx.toml):

    [tools.script.{name}]
    path = "tools/{filename}"
    timeout = 30
    # api_key = "${{{name_upper}_API_KEY}}"

  Test:
    ctx tool test tools/{filename} --param key=value
]]

tool = {{
    name = "{name}",
    version = "0.1.0",
    description = "TODO: describe what this tool does",
    parameters = {{
        {{
            name = "query",
            type = "string",
            required = true,
            description = "Input query",
        }},
    }},
}}

--- Execute the tool with the given parameters and context.
--- @param params table Validated parameters from the caller
--- @param context table Bridge to Context Harness (search, get, sources, config)
--- @return table Result to be serialized as JSON
function tool.execute(params, context)
    -- Example: search the knowledge base
    -- local results = context.search(params.query, {{ mode = "hybrid", limit = 5 }})

    -- Example: access tool config from ctx.toml
    -- local api_key = context.config.api_key

    return {{
        success = true,
        message = "TODO: implement tool logic",
        query = params.query,
    }}
end
"#,
        name = name,
        filename = filename,
        name_upper = name.to_uppercase().replace('-', "_"),
    );

    std::fs::write(&path, template)?;
    println!("Created tool: {}", path.display());
    println!();
    println!("Add to your ctx.toml:");
    println!();
    println!("  [tools.script.{}]", name);
    println!("  path = \"tools/{}\"", filename);
    println!();
    println!("Then test:");
    println!();
    println!("  ctx tool test tools/{} --param query=\"hello\"", filename);

    Ok(())
}

/// Test a tool script with sample parameters.
///
/// Loads the script, executes `tool.execute()` with the provided parameters,
/// and prints the result. Useful for development and debugging.
pub async fn test_tool(
    path: &Path,
    params: Vec<(String, String)>,
    config: &Config,
    source: Option<&str>,
) -> Result<()> {
    let script_src = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read tool script: {}", path.display()))?;

    let tool_config_extra = if let Some(name) = source {
        config
            .tools
            .script
            .get(name)
            .map(|sc| sc.extra.clone())
            .unwrap_or_default()
    } else {
        toml::Table::new()
    };

    let timeout = source
        .and_then(|name| config.tools.script.get(name))
        .map(|sc| sc.timeout)
        .unwrap_or(30);

    let name = source.unwrap_or("test").to_string();
    println!("Testing tool: {} ({})", name, path.display());

    // Build params JSON
    let mut params_json = serde_json::Map::new();
    for (k, v) in &params {
        // Try to parse as JSON value first, fall back to string
        let json_val = serde_json::from_str::<serde_json::Value>(v)
            .unwrap_or_else(|_| serde_json::Value::String(v.clone()));
        params_json.insert(k.clone(), json_val);
    }
    let params_value = serde_json::Value::Object(params_json);

    let tool_def = ToolDefinition {
        name: name.clone(),
        description: String::new(),
        parameters_schema: serde_json::json!({"type": "object", "properties": {}}),
        script_path: path.to_path_buf(),
        script_source: script_src,
        config: tool_config_extra,
        timeout,
    };

    println!("  ✓ Script loaded");

    let result = execute_tool(&tool_def, params_value, config).await?;

    println!("  ✓ Execution completed");
    println!();
    println!("Result:");

    let pretty = serde_json::to_string_pretty(&result)?;
    for line in pretty.lines() {
        println!("  {}", line);
    }

    Ok(())
}

/// List all configured tools and print their info.
pub fn list_tools(config: &Config) -> Result<()> {
    let tool_defs = load_tool_definitions(config)?;
    let tools = build_tool_list(&tool_defs);

    if tools.is_empty() {
        println!("No tools configured.");
        return Ok(());
    }

    println!("{:<24} {:<8} DESCRIPTION", "TOOL", "TYPE");
    for t in &tools {
        let type_str = if t.builtin { "built-in" } else { "lua" };
        println!("{:<24} {:<8} {}", t.name, type_str, t.description);
    }

    Ok(())
}
