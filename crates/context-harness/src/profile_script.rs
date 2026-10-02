//! Lua scripted profile runtime.
//!
//! Loads `.lua` profile scripts at startup, extracts their metadata (name,
//! description, tools, arguments), and provides runtime resolution with a
//! context bridge back into the Rust core (search, get, sources).
//!
//! # Architecture
//!
//! Profile scripts follow the same sandboxed Lua VM model as connectors and
//! tools, reusing all host APIs from [`crate::lua_runtime`]. In addition,
//! profiles receive a `context` table identical to tools:
//!
//! - `context.search(query, opts?)` — search the knowledge base
//! - `context.get(id)` — retrieve a document by UUID
//! - `context.sources()` — list connector status
//!
//! The profile-specific config from `ctx.toml` is passed as the second
//! argument to `profile.resolve(args, config, context)`.
//!
//! # Script Interface
//!
//! Every profile script defines a global `profile` table. The historical
//! `agent` global remains accepted for compatibility:
//!
//! ```lua
//! profile = {
//!     name = "my-profile",
//!     description = "Helps with tasks",
//!     tools = { "search", "get" },
//!     arguments = {
//!         { name = "topic", description = "Focus area", required = false },
//!     },
//! }
//!
//! function profile.resolve(args, config, context)
//!     return {
//!         system = "You are a helpful assistant.",
//!         messages = {},
//!     }
//! end
//! ```
//!
//! # Configuration
//!
//! ```toml
//! [profiles.script.my_profile]
//! path = "profiles/my-profile.lua"
//! timeout = 30
//! search_limit = 5
//! ```
//!
//! See the profiles documentation for the full specification.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use mlua::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::{Config, ScriptProfileConfig};
use crate::get::get_document;
use crate::lua_runtime::{json_value_to_lua, register_all_host_apis, toml_table_to_lua};
use crate::profiles::{Profile, ProfileArgument, ProfilePrompt, PromptMessage};
use crate::search::search_documents;
use crate::sources::get_sources;
use crate::traits::ToolContext;

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

/// Metadata extracted from a loaded Lua profile script.
///
/// Created at startup by [`load_profile_definitions`]. Contains everything
/// needed to register the profile and resolve its prompt when called.
#[derive(Debug, Clone)]
pub struct ProfileDefinition {
    /// Profile identifier (matches the config key in `[profiles.script.<name>]`).
    pub name: String,
    /// One-line description for profile discovery.
    pub description: String,
    /// Tools this profile uses.
    pub tools: Vec<String>,
    /// Arguments the profile accepts.
    pub arguments: Vec<ProfileArgument>,
    /// Path to the `.lua` script file.
    pub script_path: PathBuf,
    /// Raw Lua source code (cached to avoid re-reading on every call).
    pub script_source: String,
    /// Profile-specific config keys from `ctx.toml`.
    pub config: toml::Table,
    /// Maximum execution time in seconds.
    pub timeout: u64,
}

// ═══════════════════════════════════════════════════════════════════════
// Profile trait adapter
// ═══════════════════════════════════════════════════════════════════════

/// Adapter that wraps a Lua [`ProfileDefinition`] as a [`Profile`] trait object.
///
/// This allows Lua profiles to participate in unified profile dispatch
/// alongside TOML and custom Rust profiles.
pub struct LuaProfileAdapter {
    /// The underlying Lua profile definition.
    definition: ProfileDefinition,
    /// Application config needed for the context bridge.
    config: Arc<Config>,
}

impl LuaProfileAdapter {
    /// Create a new adapter wrapping a Lua profile definition.
    pub fn new(definition: ProfileDefinition, config: Arc<Config>) -> Self {
        Self { definition, config }
    }
}

#[async_trait]
impl Profile for LuaProfileAdapter {
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    fn tools(&self) -> Vec<String> {
        self.definition.tools.clone()
    }

    fn source(&self) -> &str {
        "lua"
    }

    fn arguments(&self) -> Vec<ProfileArgument> {
        self.definition.arguments.clone()
    }

    async fn resolve(&self, args: serde_json::Value, _ctx: &ToolContext) -> Result<ProfilePrompt> {
        resolve_profile(&self.definition, args, &self.config).await
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Loading
// ═══════════════════════════════════════════════════════════════════════

/// Load all profile scripts from config and extract their definitions.
///
/// For each `[profiles.script.<name>]` entry, reads the script file, creates
/// a temporary Lua VM to extract the `profile` table metadata, and converts
/// the argument declarations.
///
/// Called once at startup.
pub fn load_profile_definitions(config: &Config) -> Result<Vec<ProfileDefinition>> {
    let mut profiles = Vec::new();

    for (name, profile_config) in &config.profiles.script {
        let profile_def = load_single_profile(name, profile_config)
            .with_context(|| format!("Failed to load profile script '{}'", name))?;
        profiles.push(profile_def);
    }

    Ok(profiles)
}

/// Load a single profile script and extract its definition.
pub fn load_single_profile(
    name: &str,
    profile_config: &ScriptProfileConfig,
) -> Result<ProfileDefinition> {
    let script_src = std::fs::read_to_string(&profile_config.path).with_context(|| {
        format!(
            "Failed to read profile script: {}",
            profile_config.path.display()
        )
    })?;

    // Create a temporary Lua VM just to extract metadata
    let lua = Lua::new();
    lua.load(&script_src)
        .set_name(profile_config.path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute profile script {}: {}",
                profile_config.path.display(),
                e
            )
        })?;

    let profile_table = get_profile_table(&lua)?;

    let description: String = profile_table
        .get::<String>("description")
        .unwrap_or_else(|_| format!("Lua profile: {}", name));

    // Extract tools list
    let tools = extract_string_list(&profile_table, "tools")?;

    // Extract arguments
    let arguments = extract_arguments(&profile_table)?;

    Ok(ProfileDefinition {
        name: name.to_string(),
        description,
        tools,
        arguments,
        script_path: profile_config.path.clone(),
        script_source: script_src,
        config: profile_config.extra.clone(),
        timeout: profile_config.timeout,
    })
}

/// Return the canonical `profile` table, falling back to the legacy `agent`
/// global so existing scripts continue to work unchanged.
fn get_profile_table(lua: &Lua) -> Result<LuaTable> {
    if let Ok(profile) = lua.globals().get::<LuaTable>("profile") {
        return Ok(profile);
    }

    lua.globals().get::<LuaTable>("agent").map_err(|_| {
        anyhow::anyhow!(
            "Script must define a global 'profile' table (legacy 'agent' is also accepted)"
        )
    })
}

/// Extract a list of strings from a Lua table field.
fn extract_string_list(table: &LuaTable, key: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    if let Ok(list) = table.get::<LuaTable>(key) {
        let len = list.raw_len();
        for i in 1..=len {
            if let Ok(s) = list.raw_get::<String>(i) {
                result.push(s);
            }
        }
    }
    Ok(result)
}

/// Extract argument definitions from a Lua `profile.arguments` table.
fn extract_arguments(table: &LuaTable) -> Result<Vec<ProfileArgument>> {
    let mut result = Vec::new();
    if let Ok(args_table) = table.get::<LuaTable>("arguments") {
        let len = args_table.raw_len();
        for i in 1..=len {
            if let Ok(arg) = args_table.raw_get::<LuaTable>(i) {
                let name: String = arg.get::<String>("name").map_err(|e| {
                    anyhow::anyhow!("Argument at index {} missing 'name': {}", i, e)
                })?;
                let description: String = arg.get::<String>("description").unwrap_or_default();
                let required: bool = arg.get::<bool>("required").unwrap_or(false);
                result.push(ProfileArgument {
                    name,
                    description,
                    required,
                });
            }
        }
    }
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════
// Resolution
// ═══════════════════════════════════════════════════════════════════════

/// Resolve a profile script's prompt.
///
/// Spawns a blocking thread, creates a sandboxed Lua VM with all host APIs
/// plus the context bridge, and calls `profile.resolve(args, config, context)`.
pub async fn resolve_profile(
    profile: &ProfileDefinition,
    args: serde_json::Value,
    app_config: &Config,
) -> Result<ProfilePrompt> {
    let profile = profile.clone();
    let config = app_config.clone();

    tokio::task::spawn_blocking(move || run_lua_profile(&profile, args, &config))
        .await
        .context("Lua profile task panicked")?
}

/// Run the Lua profile synchronously on a blocking thread.
fn run_lua_profile(
    profile: &ProfileDefinition,
    args: serde_json::Value,
    config: &Config,
) -> Result<ProfilePrompt> {
    let script_dir = profile
        .script_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let lua = Lua::new();

    // Set up timeout via instruction hook
    let timeout_secs = profile.timeout;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(10_000),
        move |_lua, _debug| {
            if Instant::now() > deadline {
                Err(mlua::Error::RuntimeError(format!(
                    "profile timed out after {} seconds",
                    timeout_secs
                )))
            } else {
                Ok(mlua::VmState::Continue)
            }
        },
    );

    // Register all shared host APIs
    let log_name = format!("profile:{}", profile.name);
    register_all_host_apis(&lua, &log_name, &script_dir)?;

    // Register context bridge (search, get, sources)
    register_profile_context_bridge(&lua, config)?;

    // Load and execute the script
    lua.load(&profile.script_source)
        .set_name(profile.script_path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute profile script {}: {}",
                profile.script_path.display(),
                e
            )
        })?;

    // Get profile.resolve function (or legacy agent.resolve).
    let profile_table = get_profile_table(&lua)?;

    let resolve_fn: LuaFunction = profile_table
        .get::<LuaFunction>("resolve")
        .map_err(|e| anyhow::anyhow!("profile.resolve function not defined: {}", e))?;

    // Convert args to Lua
    let args_lua = json_value_to_lua(&lua, &args)?;

    // Convert profile config to Lua
    let config_lua = toml_table_to_lua(&lua, &profile.config)?;

    // Get the context table we already registered
    let context: LuaTable = lua
        .globals()
        .get::<LuaTable>("context")
        .map_err(|e| anyhow::anyhow!("context table missing: {}", e))?;

    // Call profile.resolve(args, config, context)
    let result: LuaValue = resolve_fn
        .call::<LuaValue>((args_lua, config_lua, context))
        .map_err(|e| {
            anyhow::anyhow!(
                "profile.resolve() failed in '{}': {}",
                profile.script_path.display(),
                e
            )
        })?;

    // Convert result to ProfilePrompt
    lua_result_to_profile_prompt(result)
}

/// Convert the Lua `profile.resolve()` return value to a [`ProfilePrompt`].
///
/// Expected shape:
/// ```lua
/// {
///     system = "You are...",
///     messages = {
///         { role = "assistant", content = "I'm ready..." },
///     }
/// }
/// ```
fn lua_result_to_profile_prompt(value: LuaValue) -> Result<ProfilePrompt> {
    match value {
        LuaValue::Table(table) => {
            let system: String = table.get::<String>("system").map_err(|_| {
                anyhow::anyhow!("profile.resolve() must return a table with 'system' field")
            })?;

            // Extract tools override (optional — a profile might narrow its own list)
            let tools = if let Ok(tools_table) = table.get::<LuaTable>("tools") {
                let mut result = Vec::new();
                let len = tools_table.raw_len();
                for i in 1..=len {
                    if let Ok(s) = tools_table.raw_get::<String>(i) {
                        result.push(s);
                    }
                }
                result
            } else {
                vec![]
            };

            // Extract messages (optional)
            let messages = if let Ok(msgs_table) = table.get::<LuaTable>("messages") {
                let mut result = Vec::new();
                let len = msgs_table.raw_len();
                for i in 1..=len {
                    if let Ok(msg) = msgs_table.raw_get::<LuaTable>(i) {
                        let role: String = msg
                            .get::<String>("role")
                            .unwrap_or_else(|_| "assistant".to_string());
                        let content: String = msg.get::<String>("content").unwrap_or_default();
                        result.push(PromptMessage { role, content });
                    }
                }
                result
            } else {
                vec![]
            };

            Ok(ProfilePrompt {
                system,
                tools,
                messages,
            })
        }
        _ => {
            anyhow::bail!(
                "profile.resolve() must return a table, got {:?}",
                value.type_name()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Context Bridge
// ═══════════════════════════════════════════════════════════════════════

/// Register the `context` table in the Lua VM for profile scripts.
///
/// Provides `context.search`, `context.get`, and `context.sources`.
/// Uses the same bridge pattern as tool scripts.
fn register_profile_context_bridge(lua: &Lua, config: &Config) -> LuaResult<()> {
    let ctx = lua.create_table()?;

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
                    search_documents(
                        &cfg,
                        &query,
                        &mode,
                        source.as_deref(),
                        None,
                        Some(limit),
                        false,
                    )
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
fn search_results_to_lua(
    lua: &Lua,
    results: &[crate::search::SearchResultItem],
) -> LuaResult<LuaTable> {
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
fn doc_response_to_lua(lua: &Lua, doc: &crate::get::DocumentResponse) -> LuaResult<LuaTable> {
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
fn sources_to_lua(lua: &Lua, sources: &[crate::sources::SourceStatus]) -> LuaResult<LuaTable> {
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
// CLI: scaffold & test
// ═══════════════════════════════════════════════════════════════════════

/// Scaffold a new profile script from a template.
///
/// Creates `profiles/<name>.lua` with a commented template showing the
/// profile interface and available host APIs.
pub fn scaffold_profile(name: &str) -> Result<()> {
    let dir = Path::new("profiles");
    std::fs::create_dir_all(dir)?;

    let filename = format!("{}.lua", name.replace('_', "-"));
    let path = dir.join(&filename);

    if path.exists() {
        bail!("Profile script already exists: {}", path.display());
    }

    let template = format!(
        r#"--[[
  Context Harness Profile: {name}

  Configuration (add to ctx.toml):

    [profiles.script.{name}]
    path = "profiles/{filename}"
    timeout = 30
    # search_limit = 5

  Test:
    ctx profile test {name} --arg key=value
]]

profile = {{
    name = "{name}",
    description = "TODO: describe what this profile does",
    tools = {{ "search", "get" }},
    arguments = {{
        {{
            name = "topic",
            description = "Focus area for this session",
            required = false,
        }},
    }},
}}

--- Resolve the profile's prompt for a conversation.
--- @param args table User-provided argument values
--- @param config table Profile-specific config from ctx.toml
--- @param context table Bridge to Context Harness (search, get, sources)
--- @return table Resolved prompt with system, tools, and messages
function profile.resolve(args, config, context)
    local topic = args.topic or "general"

    -- Example: pre-search for relevant context
    -- local results = context.search(topic, {{ mode = "keyword", limit = 5 }})
    -- local context_text = ""
    -- for _, r in ipairs(results) do
    --     local doc = context.get(r.id)
    --     context_text = context_text .. "\n---\n" .. doc.body
    -- end

    return {{
        system = string.format([[
You are a helpful assistant focused on %s.

Use the search tool to find relevant documentation and ground
your responses in the indexed knowledge base.
        ]], topic),
        messages = {{
            {{
                role = "assistant",
                content = string.format(
                    "I'm ready to help with %s. What would you like to know?",
                    topic
                ),
            }},
        }},
    }}
end

return profile
"#,
        name = name,
        filename = filename,
    );

    std::fs::write(&path, template)?;
    println!("Created profile: {}", path.display());
    println!();
    println!("Add to your ctx.toml:");
    println!();
    println!("  [profiles.script.{}]", name);
    println!("  path = \"profiles/{}\"", filename);
    println!();
    println!("Then test:");
    println!();
    println!("  ctx profile test {} --arg topic=\"deployment\"", name);

    Ok(())
}

/// Test a profile script by resolving its prompt.
///
/// Loads the script, executes `profile.resolve()` with the provided arguments,
/// and prints the resulting system prompt, tools, and messages.
pub async fn test_profile(name: &str, args: Vec<(String, String)>, config: &Config) -> Result<()> {
    let profile_config = config
        .profiles
        .script
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found in config", name))?;

    let profile_def = load_single_profile(name, profile_config)?;

    println!("Profile: {}", profile_def.name);
    println!("Source: lua ({})", profile_def.script_path.display());
    println!(
        "Tools: {}",
        if profile_def.tools.is_empty() {
            "(none defined)".to_string()
        } else {
            profile_def.tools.join(", ")
        }
    );
    println!();

    // Build args JSON
    let mut args_json = serde_json::Map::new();
    for (k, v) in &args {
        let json_val = serde_json::from_str::<serde_json::Value>(v)
            .unwrap_or_else(|_| serde_json::Value::String(v.clone()));
        args_json.insert(k.clone(), json_val);
    }
    let args_value = serde_json::Value::Object(args_json);

    let start = Instant::now();
    let prompt = resolve_profile(&profile_def, args_value, config).await?;
    let elapsed = start.elapsed();

    println!("System prompt ({} chars):", prompt.system.len());
    for line in prompt.system.lines() {
        println!("  {}", line);
    }

    if !prompt.tools.is_empty() {
        println!();
        println!("Tools override: {}", prompt.tools.join(", "));
    }

    if !prompt.messages.is_empty() {
        println!();
        println!("Messages ({}):", prompt.messages.len());
        for msg in &prompt.messages {
            println!("  [{}] {}", msg.role, msg.content);
        }
    }

    println!();
    println!("Resolved in {:.0?}", elapsed);

    Ok(())
}

/// List all configured profiles and print their info.
#[allow(dead_code)] // Retained for library callers; CLI uses the combined catalog.
pub fn list_profiles(config: &Config) -> Result<()> {
    let mut count = 0;

    println!(
        "{:<24} {:<8} {:<44} TOOLS",
        "PROFILE", "TYPE", "DESCRIPTION"
    );

    // TOML profiles
    for (name, cfg) in &config.profiles.inline {
        println!(
            "{:<24} {:<8} {:<44} {}",
            name,
            "toml",
            truncate(&cfg.description, 44),
            cfg.tools.join(", ")
        );
        count += 1;
    }

    // Lua profiles
    let lua_defs = load_profile_definitions(config)?;
    for def in &lua_defs {
        println!(
            "{:<24} {:<8} {:<44} {}",
            def.name,
            "lua",
            truncate(&def.description, 44),
            def.tools.join(", ")
        );
        count += 1;
    }

    if count == 0 {
        println!("No profiles configured.");
    }

    Ok(())
}

/// Truncate a string to fit in a column.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
