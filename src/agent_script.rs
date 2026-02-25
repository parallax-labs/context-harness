//! Lua scripted agent runtime.
//!
//! Loads `.lua` agent scripts at startup, extracts their metadata (name,
//! description, tools, arguments), and provides runtime resolution with a
//! context bridge back into the Rust core (search, get, sources).
//!
//! # Architecture
//!
//! Agent scripts follow the same sandboxed Lua VM model as connectors and
//! tools, reusing all host APIs from [`crate::lua_runtime`]. In addition,
//! agents receive a `context` table identical to tools:
//!
//! - `context.search(query, opts?)` — search the knowledge base
//! - `context.get(id)` — retrieve a document by UUID
//! - `context.sources()` — list connector status
//!
//! The agent-specific config from `ctx.toml` is passed as the second
//! argument to `agent.resolve(args, config, context)`.
//!
//! # Script Interface
//!
//! Every agent script defines a global `agent` table:
//!
//! ```lua
//! agent = {
//!     name = "my-agent",
//!     description = "Helps with tasks",
//!     tools = { "search", "get" },
//!     arguments = {
//!         { name = "topic", description = "Focus area", required = false },
//!     },
//! }
//!
//! function agent.resolve(args, config, context)
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
//! [agents.script.my_agent]
//! path = "agents/my-agent.lua"
//! timeout = 30
//! search_limit = 5
//! ```
//!
//! See `docs/AGENTS.md` for the full specification.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use mlua::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agents::{Agent, AgentArgument, AgentPrompt, PromptMessage};
use crate::config::{Config, ScriptAgentConfig};
use crate::get::get_document;
use crate::lua_runtime::{json_value_to_lua, register_all_host_apis, toml_table_to_lua};
use crate::search::search_documents;
use crate::sources::get_sources;
use crate::traits::ToolContext;

// ═══════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════

/// Metadata extracted from a loaded Lua agent script.
///
/// Created at startup by [`load_agent_definitions`]. Contains everything
/// needed to register the agent and resolve its prompt when called.
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    /// Agent identifier (matches the config key in `[agents.script.<name>]`).
    pub name: String,
    /// One-line description for agent discovery.
    pub description: String,
    /// Tools this agent uses.
    pub tools: Vec<String>,
    /// Arguments the agent accepts.
    pub arguments: Vec<AgentArgument>,
    /// Path to the `.lua` script file.
    pub script_path: PathBuf,
    /// Raw Lua source code (cached to avoid re-reading on every call).
    pub script_source: String,
    /// Agent-specific config keys from `ctx.toml`.
    pub config: toml::Table,
    /// Maximum execution time in seconds.
    pub timeout: u64,
}

// ═══════════════════════════════════════════════════════════════════════
// Agent trait adapter
// ═══════════════════════════════════════════════════════════════════════

/// Adapter that wraps a Lua [`AgentDefinition`] as an [`Agent`] trait object.
///
/// This allows Lua agents to participate in the unified agent dispatch
/// alongside TOML and custom Rust agents.
pub struct LuaAgentAdapter {
    /// The underlying Lua agent definition.
    definition: AgentDefinition,
    /// Application config needed for the context bridge.
    config: Arc<Config>,
}

impl LuaAgentAdapter {
    /// Create a new adapter wrapping a Lua agent definition.
    pub fn new(definition: AgentDefinition, config: Arc<Config>) -> Self {
        Self { definition, config }
    }
}

#[async_trait]
impl Agent for LuaAgentAdapter {
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

    fn arguments(&self) -> Vec<AgentArgument> {
        self.definition.arguments.clone()
    }

    async fn resolve(&self, args: serde_json::Value, _ctx: &ToolContext) -> Result<AgentPrompt> {
        resolve_agent(&self.definition, args, &self.config).await
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Loading
// ═══════════════════════════════════════════════════════════════════════

/// Load all agent scripts from config and extract their definitions.
///
/// For each `[agents.script.<name>]` entry, reads the script file, creates
/// a temporary Lua VM to extract the `agent` table metadata, and converts
/// the argument declarations.
///
/// Called once at startup.
pub fn load_agent_definitions(config: &Config) -> Result<Vec<AgentDefinition>> {
    let mut agents = Vec::new();

    for (name, agent_config) in &config.agents.script {
        let agent_def = load_single_agent(name, agent_config)
            .with_context(|| format!("Failed to load agent script '{}'", name))?;
        agents.push(agent_def);
    }

    Ok(agents)
}

/// Load a single agent script and extract its definition.
fn load_single_agent(name: &str, agent_config: &ScriptAgentConfig) -> Result<AgentDefinition> {
    let script_src = std::fs::read_to_string(&agent_config.path).with_context(|| {
        format!(
            "Failed to read agent script: {}",
            agent_config.path.display()
        )
    })?;

    // Create a temporary Lua VM just to extract metadata
    let lua = Lua::new();
    lua.load(&script_src)
        .set_name(agent_config.path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute agent script {}: {}",
                agent_config.path.display(),
                e
            )
        })?;

    let agent_table: LuaTable = lua
        .globals()
        .get::<LuaTable>("agent")
        .map_err(|e| anyhow::anyhow!("Script must define a global 'agent' table: {}", e))?;

    let description: String = agent_table
        .get::<String>("description")
        .unwrap_or_else(|_| format!("Lua agent: {}", name));

    // Extract tools list
    let tools = extract_string_list(&agent_table, "tools")?;

    // Extract arguments
    let arguments = extract_arguments(&agent_table)?;

    Ok(AgentDefinition {
        name: name.to_string(),
        description,
        tools,
        arguments,
        script_path: agent_config.path.clone(),
        script_source: script_src,
        config: agent_config.extra.clone(),
        timeout: agent_config.timeout,
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

/// Extract argument definitions from a Lua `agent.arguments` table.
fn extract_arguments(table: &LuaTable) -> Result<Vec<AgentArgument>> {
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
                result.push(AgentArgument {
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

/// Resolve an agent script's prompt.
///
/// Spawns a blocking thread, creates a sandboxed Lua VM with all host APIs
/// plus the context bridge, and calls `agent.resolve(args, config, context)`.
pub async fn resolve_agent(
    agent: &AgentDefinition,
    args: serde_json::Value,
    app_config: &Config,
) -> Result<AgentPrompt> {
    let agent = agent.clone();
    let config = app_config.clone();

    tokio::task::spawn_blocking(move || run_lua_agent(&agent, args, &config))
        .await
        .context("Lua agent task panicked")?
}

/// Run the Lua agent synchronously on a blocking thread.
fn run_lua_agent(
    agent: &AgentDefinition,
    args: serde_json::Value,
    config: &Config,
) -> Result<AgentPrompt> {
    let script_dir = agent
        .script_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let lua = Lua::new();

    // Set up timeout via instruction hook
    let timeout_secs = agent.timeout;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(10_000),
        move |_lua, _debug| {
            if Instant::now() > deadline {
                Err(mlua::Error::RuntimeError(format!(
                    "agent timed out after {} seconds",
                    timeout_secs
                )))
            } else {
                Ok(mlua::VmState::Continue)
            }
        },
    );

    // Register all shared host APIs
    let log_name = format!("agent:{}", agent.name);
    register_all_host_apis(&lua, &log_name, &script_dir)?;

    // Register context bridge (search, get, sources)
    register_agent_context_bridge(&lua, config)?;

    // Load and execute the script
    lua.load(&agent.script_source)
        .set_name(agent.script_path.to_string_lossy())
        .exec()
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to execute agent script {}: {}",
                agent.script_path.display(),
                e
            )
        })?;

    // Get agent.resolve function
    let agent_table: LuaTable = lua
        .globals()
        .get::<LuaTable>("agent")
        .map_err(|e| anyhow::anyhow!("Script must define a global 'agent' table: {}", e))?;

    let resolve_fn: LuaFunction = agent_table
        .get::<LuaFunction>("resolve")
        .map_err(|e| anyhow::anyhow!("agent.resolve function not defined: {}", e))?;

    // Convert args to Lua
    let args_lua = json_value_to_lua(&lua, &args)?;

    // Convert agent config to Lua
    let config_lua = toml_table_to_lua(&lua, &agent.config)?;

    // Get the context table we already registered
    let context: LuaTable = lua
        .globals()
        .get::<LuaTable>("context")
        .map_err(|e| anyhow::anyhow!("context table missing: {}", e))?;

    // Call agent.resolve(args, config, context)
    let result: LuaValue = resolve_fn
        .call::<LuaValue>((args_lua, config_lua, context))
        .map_err(|e| {
            anyhow::anyhow!(
                "agent.resolve() failed in '{}': {}",
                agent.script_path.display(),
                e
            )
        })?;

    // Convert result to AgentPrompt
    lua_result_to_agent_prompt(result)
}

/// Convert the Lua `agent.resolve()` return value to an [`AgentPrompt`].
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
fn lua_result_to_agent_prompt(value: LuaValue) -> Result<AgentPrompt> {
    match value {
        LuaValue::Table(table) => {
            let system: String = table.get::<String>("system").map_err(|_| {
                anyhow::anyhow!("agent.resolve() must return a table with 'system' field")
            })?;

            // Extract tools override (optional — agent might narrow its own list)
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

            Ok(AgentPrompt {
                system,
                tools,
                messages,
            })
        }
        _ => {
            anyhow::bail!(
                "agent.resolve() must return a table, got {:?}",
                value.type_name()
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Context Bridge
// ═══════════════════════════════════════════════════════════════════════

/// Register the `context` table in the Lua VM for agent scripts.
///
/// Provides `context.search`, `context.get`, and `context.sources`.
/// Uses the same bridge pattern as tool scripts.
fn register_agent_context_bridge(lua: &Lua, config: &Config) -> LuaResult<()> {
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

/// Scaffold a new agent script from a template.
///
/// Creates `agents/<name>.lua` with a commented template showing the
/// agent interface and available host APIs.
pub fn scaffold_agent(name: &str) -> Result<()> {
    let dir = Path::new("agents");
    std::fs::create_dir_all(dir)?;

    let filename = format!("{}.lua", name.replace('_', "-"));
    let path = dir.join(&filename);

    if path.exists() {
        bail!("Agent script already exists: {}", path.display());
    }

    let template = format!(
        r#"--[[
  Context Harness Agent: {name}

  Configuration (add to ctx.toml):

    [agents.script.{name}]
    path = "agents/{filename}"
    timeout = 30
    # search_limit = 5

  Test:
    ctx agent test {name} --arg key=value
]]

agent = {{
    name = "{name}",
    description = "TODO: describe what this agent does",
    tools = {{ "search", "get" }},
    arguments = {{
        {{
            name = "topic",
            description = "Focus area for this session",
            required = false,
        }},
    }},
}}

--- Resolve the agent's prompt for a conversation.
--- @param args table User-provided argument values
--- @param config table Agent-specific config from ctx.toml
--- @param context table Bridge to Context Harness (search, get, sources)
--- @return table Resolved prompt with system, tools, and messages
function agent.resolve(args, config, context)
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

return agent
"#,
        name = name,
        filename = filename,
    );

    std::fs::write(&path, template)?;
    println!("Created agent: {}", path.display());
    println!();
    println!("Add to your ctx.toml:");
    println!();
    println!("  [agents.script.{}]", name);
    println!("  path = \"agents/{}\"", filename);
    println!();
    println!("Then test:");
    println!();
    println!("  ctx agent test {} --arg topic=\"deployment\"", name);

    Ok(())
}

/// Test an agent script by resolving its prompt.
///
/// Loads the script, executes `agent.resolve()` with the provided arguments,
/// and prints the resulting system prompt, tools, and messages.
pub async fn test_agent(name: &str, args: Vec<(String, String)>, config: &Config) -> Result<()> {
    let agent_config = config
        .agents
        .script
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found in config", name))?;

    let agent_def = load_single_agent(name, agent_config)?;

    println!("Agent: {}", agent_def.name);
    println!("Source: lua ({})", agent_def.script_path.display());
    println!(
        "Tools: {}",
        if agent_def.tools.is_empty() {
            "(none defined)".to_string()
        } else {
            agent_def.tools.join(", ")
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
    let prompt = resolve_agent(&agent_def, args_value, config).await?;
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

/// List all configured agents and print their info.
pub fn list_agents(config: &Config) -> Result<()> {
    let mut count = 0;

    println!("{:<24} {:<8} {:<44} TOOLS", "AGENT", "TYPE", "DESCRIPTION");

    // TOML agents
    for (name, cfg) in &config.agents.inline {
        println!(
            "{:<24} {:<8} {:<44} {}",
            name,
            "toml",
            truncate(&cfg.description, 44),
            cfg.tools.join(", ")
        );
        count += 1;
    }

    // Lua agents
    let lua_defs = load_agent_definitions(config)?;
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
        println!("No agents configured.");
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
