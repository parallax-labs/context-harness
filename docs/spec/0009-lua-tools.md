# Lua MCP Tool Extensions — Design & Specification

This document specifies the design for runtime-loadable Lua tools in
Context Harness. Lua scripts define custom MCP tools that AI agents
(Cursor, Claude, browser LLMs) can discover and call via the HTTP server —
without recompiling Rust.

**Status:** Implemented  
**Author:** Parker Jones  
**Created:** 2026-02-22  
**Depends on:** `connector_script.rs` (Lua VM runtime, host APIs)

---

## 1. Motivation

Context Harness currently exposes three hardcoded MCP tools: `search`,
`get`, and `sources`. These are **read-only** — the LLM can query the
knowledge base but cannot take actions. Adding a new tool today requires
writing Rust, compiling, and releasing a new binary.

Lua tool scripts complete the loop: connectors read data **into** the
knowledge base; tools let agents **act on** that data. Together they form
a full bidirectional bridge between AI agents and external systems.

### Use Cases

| Use Case | Connector (read) | Tool (write/act) |
|----------|-------------------|-------------------|
| Jira | Ingest issues → search | Create/update tickets |
| Slack | Ingest threads → search | Post messages, create threads |
| Confluence | Ingest pages → search | Create/update pages |
| GitHub | Ingest issues/PRs → search | Create issues, post comments |
| Internal APIs | Ingest docs → search | Trigger deploys, run queries |
| Notifications | — | Send email/Slack alerts based on search results |

### Why Lua (Again)

The Lua connector runtime already provides everything a tool script needs:
sandboxed VM, HTTP client, JSON encoding, base64, crypto, logging, and
timeout protection. The only new components are the **parameter schema**
declaration and the **context bridge** that gives tools access to the
knowledge base.

---

## 2. Architecture Overview

```
ctx.toml                               HTTP Server
┌──────────────────────┐     ┌──────────────────────────────┐
│ [tools.script.X]     │     │                              │
│   path = "tools/x.lua" ──────▶ startup: load script,      │
│   api_key = "${KEY}"  │     │   extract tool.parameters,   │
│                       │     │   register POST /tools/X     │
│ [tools.script.Y]     │     │                              │
│   path = "tools/y.lua" ──────▶ register POST /tools/Y     │
└──────────────────────┘     │                              │
                             │ GET /tools/list ◄──── returns │
                             │   all tools (built-in + Lua)  │
                             │   with JSON schemas           │
                             └──────────────────────────────┘
                                        │
                            Agent calls POST /tools/X
                                        │
                                        ▼
                             ┌──────────────────────┐
                             │ Lua VM (blocking)     │
                             │                       │
                             │ tool.execute(params,  │
                             │   context) → result   │
                             │                       │
                             │ context.search(q)     │
                             │ context.get(id)       │
                             │ context.sources()     │
                             └──────────────────────┘
```

### Data Flow (tool invocation)

1. Agent discovers tools via `GET /tools/list`
2. Agent calls `POST /tools/{name}` with JSON parameters
3. Server validates parameters against the tool's schema
4. Server creates a Lua VM, registers host APIs + context bridge
5. Server calls `tool.execute(params, context)` on a blocking thread
6. Script can call `context.search()`, `context.get()`, etc.
7. Script returns a result table
8. Server serializes the result as JSON and returns it

---

## 3. User-Facing API

### 3.1 Configuration

Tool scripts are configured under `[tools.script.<name>]`. The `path`
key points to a `.lua` file. All other keys become the `config` table
accessible via `context.config` inside the script.

```toml
[tools.script.create_jira_ticket]
path = "tools/create-jira-ticket.lua"
timeout = 30
url = "https://mycompany.atlassian.net"
email = "bot@company.com"
api_token = "${JIRA_API_TOKEN}"

[tools.script.post_slack]
path = "tools/post-slack.lua"
timeout = 10
webhook_url = "${SLACK_WEBHOOK_URL}"
```

**Environment variable expansion:** Values containing `${VAR_NAME}` are
expanded from the process environment before being passed to Lua.

### 3.2 CLI

```bash
# Scaffold a new tool from a template
ctx tool init create-jira-ticket

# Test a tool script with sample parameters (without affecting the server)
ctx tool test tools/create-jira-ticket.lua \
  --param title="Fix auth bug" \
  --param body="The login flow breaks when..." \
  --source create_jira_ticket

# List all registered tools
ctx tool list

# Start the server (serves built-in + Lua tools)
ctx serve mcp
```

### 3.3 Tool Discovery Endpoint

`GET /tools/list` returns all available tools with their parameter
schemas in OpenAI function-calling format:

```json
{
  "tools": [
    {
      "name": "search",
      "description": "Search the knowledge base",
      "builtin": true,
      "parameters": {
        "type": "object",
        "properties": {
          "query": { "type": "string", "description": "Search query" },
          "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"] },
          "limit": { "type": "integer", "default": 12 }
        },
        "required": ["query"]
      }
    },
    {
      "name": "create_jira_ticket",
      "description": "Create a Jira ticket from a specification",
      "builtin": false,
      "parameters": {
        "type": "object",
        "properties": {
          "title": { "type": "string", "description": "Ticket title" },
          "body": { "type": "string", "description": "Description (markdown)" },
          "project": { "type": "string", "description": "Project key", "default": "ENG" }
        },
        "required": ["title", "body"]
      }
    }
  ]
}
```

This is the same schema format used by OpenAI function calling and
MCP tool definitions, so agents can use the tools without any adapter.

### 3.4 Tool Invocation Endpoint

`POST /tools/{name}` calls a tool with the given parameters:

**Request:**

```json
{
  "title": "Fix auth bug",
  "body": "The login flow breaks when the token expires...",
  "project": "ENG"
}
```

**Response (success):**

```json
{
  "result": {
    "ticket_key": "ENG-1234",
    "url": "https://mycompany.atlassian.net/browse/ENG-1234",
    "message": "Created ENG-1234: Fix auth bug"
  }
}
```

**Response (error from script):**

```json
{
  "error": {
    "code": "tool_error",
    "message": "Jira API returned 403: Forbidden"
  }
}
```

---

## 4. Lua Script Interface

### 4.1 Contract

Every tool script MUST define a global `tool` table with:
- `name` (string) — tool identifier (matches the config key)
- `description` (string) — one-line description for agent discovery
- `parameters` (table) — array of parameter definitions
- `execute` (function) — the tool implementation

```lua
tool = {
    name = "create_jira_ticket",
    version = "0.1.0",
    description = "Create a Jira ticket from a specification",
    parameters = {
        {
            name = "title",
            type = "string",
            required = true,
            description = "Ticket title",
        },
        {
            name = "body",
            type = "string",
            required = true,
            description = "Ticket description (markdown)",
        },
        {
            name = "project",
            type = "string",
            required = false,
            description = "Project key",
            default = "ENG",
        },
        {
            name = "priority",
            type = "string",
            required = false,
            description = "Priority level (low, medium, high, critical)",
            default = "medium",
            enum = { "low", "medium", "high", "critical" },
        },
    },
}

function tool.execute(params, context)
    -- params: table of validated parameters
    -- context: bridge to Context Harness (see §4.3)
    -- return: table (serialized as JSON response)
end
```

### 4.2 Parameter Schema

Each parameter in `tool.parameters` is a table with these fields:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | **yes** | Parameter name (JSON key) |
| `type` | string | **yes** | JSON type: `"string"`, `"integer"`, `"number"`, `"boolean"`, `"array"`, `"object"` |
| `required` | boolean | no | Whether the parameter is required (default: `false`) |
| `description` | string | no | Description for agent discovery |
| `default` | any | no | Default value if not provided |
| `enum` | table | no | Array of allowed values |

The host converts this to OpenAI function-calling JSON Schema:

```lua
-- Lua declaration:
{ name = "priority", type = "string", enum = { "low", "medium", "high" } }

-- Becomes JSON Schema:
{ "type": "string", "enum": ["low", "medium", "high"] }
```

### 4.3 Context Bridge

The second argument to `tool.execute()` is a `context` table providing
access to the Context Harness knowledge base and tool configuration:

```lua
function tool.execute(params, context)
    -- Search the knowledge base
    local results = context.search("deployment runbook", {
        mode = "hybrid",   -- optional, default "keyword"
        limit = 5,         -- optional, default 12
        source = "git",    -- optional, filter by connector source
    })
    -- results = {{ id = "...", score = 0.92, title = "...", snippet = "..." }, ...}

    -- Get a full document
    local doc = context.get("abc-123-uuid")
    -- doc = { id = "...", title = "...", body = "...", chunks = {...} }

    -- List connectors
    local sources = context.sources()
    -- sources = {{ name = "filesystem", configured = true, healthy = true }, ...}

    -- Access tool config from ctx.toml
    local api_url = context.config.url
    local token = context.config.api_token

    -- Return result
    return { success = true, message = "Done" }
end
```

**Implementation:** The context bridge functions call back into the Rust
core (via `search::search_documents`, `get::get_document`,
`sources::get_sources`) on the same blocking thread. They block until
the async operation completes via `tokio::runtime::Handle::block_on`.

### 4.4 Return Value

The return value from `tool.execute()` is serialized as JSON under the
`"result"` key in the HTTP response. It can be any Lua value that maps
to valid JSON:

```lua
-- Simple success
return { success = true, message = "Ticket created" }

-- Structured data
return {
    ticket_key = "ENG-1234",
    url = "https://example.com/ENG-1234",
    assignee = "jane.smith",
}

-- Error (returned as 200 with error info — the script handled it)
return { error = "Jira API returned 403: Forbidden" }
```

If `tool.execute()` raises a Lua error (unhandled exception), the server
returns a 500 with the error message in the standard error schema.

### 4.5 Side Effects & Safety

Lua tools are inherently **side-effectful** — they create tickets, post
messages, trigger deployments. The design intentionally does NOT add
confirmation prompts or dry-run modes at the server level. Safety is
the responsibility of:

1. **The agent** — LLMs should confirm destructive actions with the user
2. **The script** — scripts can implement their own safeguards
3. **The operator** — only configure tools you trust in `ctx.toml`

A future version may add an `approve` flag to require human confirmation
before executing tools with side effects (see §9.3).

---

## 5. Host APIs

Tool scripts have access to the **same host APIs** as connector scripts
(see `LUA_CONNECTORS.md` §4):

| Module | Functions |
|--------|-----------|
| `http` | `get(url, opts?)`, `post(url, body, opts?)`, `put(url, body, opts?)` |
| `json` | `parse(str)`, `encode(table)` |
| `env` | `get(name)` |
| `log` | `info(msg)`, `warn(msg)`, `error(msg)`, `debug(msg)` |
| `fs` | `read(path)`, `list(dir, glob?)` — sandboxed to script directory |
| `base64` | `encode(str)`, `decode(str)` |
| `crypto` | `sha256(data)`, `hmac_sha256(key, data)` |
| `sleep` | `sleep(seconds)` |

**In addition**, tool scripts receive the `context` bridge (§4.3) which
is not available to connector scripts (connectors produce data; tools
consume it).

---

## 6. Complete Example: Create Jira Ticket

```lua
--[[
  Context Harness Tool: create_jira_ticket

  Creates a Jira ticket with optional RAG-enriched description.
  If the body mentions technical concepts, the tool searches the
  knowledge base for related documentation and appends links.

  Configuration:
    [tools.script.create_jira_ticket]
    path = "tools/create-jira-ticket.lua"
    url = "https://mycompany.atlassian.net"
    email = "bot@company.com"
    api_token = "${JIRA_API_TOKEN}"
]]

tool = {
    name = "create_jira_ticket",
    version = "1.0.0",
    description = "Create a Jira ticket, optionally enriched with related docs",
    parameters = {
        {
            name = "title",
            type = "string",
            required = true,
            description = "Ticket title",
        },
        {
            name = "body",
            type = "string",
            required = true,
            description = "Ticket description (markdown)",
        },
        {
            name = "project",
            type = "string",
            required = false,
            description = "Jira project key",
            default = "ENG",
        },
        {
            name = "priority",
            type = "string",
            required = false,
            description = "Priority",
            default = "Medium",
            enum = { "Lowest", "Low", "Medium", "High", "Highest" },
        },
        {
            name = "enrich",
            type = "boolean",
            required = false,
            description = "Search knowledge base and append related docs",
            default = true,
        },
    },
}

function tool.execute(params, context)
    local description = params.body

    -- Optionally enrich with related documentation
    if params.enrich then
        local results = context.search(params.title, {
            mode = "hybrid",
            limit = 3,
        })

        if #results > 0 then
            description = description .. "\n\n---\n\n*Related documentation:*\n"
            for _, r in ipairs(results) do
                if r.source_url then
                    description = description .. "- [" .. r.title .. "]("
                        .. r.source_url .. ") (score: "
                        .. string.format("%.2f", r.score) .. ")\n"
                else
                    description = description .. "- " .. r.title
                        .. " (score: " .. string.format("%.2f", r.score) .. ")\n"
                end
            end
        end
    end

    -- Create the ticket via Jira REST API
    local auth = base64.encode(context.config.email .. ":" .. context.config.api_token)

    local resp = http.post(context.config.url .. "/rest/api/3/issue", json.encode({
        fields = {
            project = { key = params.project },
            summary = params.title,
            description = {
                type = "doc",
                version = 1,
                content = {
                    {
                        type = "paragraph",
                        content = {
                            { type = "text", text = description },
                        },
                    },
                },
            },
            issuetype = { name = "Task" },
            priority = { name = params.priority },
        },
    }), {
        headers = {
            ["Authorization"] = "Basic " .. auth,
            ["Content-Type"] = "application/json",
            ["Accept"] = "application/json",
        },
    })

    if not resp.ok then
        log.error("Jira API error: HTTP " .. resp.status .. " " .. resp.body)
        return {
            success = false,
            error = "Jira API returned " .. resp.status,
            details = resp.body,
        }
    end

    local ticket = resp.json
    local ticket_url = context.config.url .. "/browse/" .. ticket.key

    log.info("Created ticket: " .. ticket.key)

    return {
        success = true,
        ticket_key = ticket.key,
        url = ticket_url,
        message = "Created " .. ticket.key .. ": " .. params.title,
    }
end
```

---

## 7. Rust Implementation

### 7.1 New Module: `tool_script.rs`

Responsible for:

1. Loading tool scripts at server startup
2. Extracting `tool.name`, `tool.description`, `tool.parameters`
3. Converting Lua parameter definitions to OpenAI JSON Schema
4. Registering dynamic Axum routes at `POST /tools/{name}`
5. Executing `tool.execute(params, context)` on a blocking thread
6. Providing the `context` bridge to the Lua VM

```rust
// Pseudocode — actual implementation will follow this structure

/// Metadata extracted from a loaded Lua tool script.
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,  // OpenAI JSON Schema
    pub script_path: PathBuf,
    pub script_source: String,
    pub config: toml::Table,
    pub timeout: u64,
}

/// Load all tool scripts from config and extract their definitions.
pub fn load_tool_definitions(config: &Config) -> Result<Vec<ToolDefinition>> {
    let mut tools = Vec::new();
    for (name, tool_config) in &config.tools.script {
        let script_src = std::fs::read_to_string(&tool_config.path)?;

        // Create a temporary Lua VM just to extract metadata
        let lua = Lua::new();
        lua.load(&script_src).exec()?;

        let tool_table: Table = lua.globals().get("tool")?;
        let description: String = tool_table.get("description")?;
        let params: Table = tool_table.get("parameters")?;

        let schema = lua_params_to_json_schema(params)?;

        tools.push(ToolDefinition {
            name: name.clone(),
            description,
            parameters_schema: schema,
            script_path: tool_config.path.clone(),
            script_source: script_src,
            config: tool_config.extra.clone(),
            timeout: tool_config.timeout,
        });
    }
    Ok(tools)
}

/// Execute a tool script with the given parameters.
pub async fn execute_tool(
    tool: &ToolDefinition,
    params: serde_json::Value,
    app_config: &Config,
) -> Result<serde_json::Value> {
    let tool = tool.clone();
    let config = app_config.clone();

    tokio::task::spawn_blocking(move || {
        run_lua_tool(&tool, params, &config)
    }).await?
}
```

### 7.2 Config Changes

```rust
// In config.rs — add tools section

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub db: DbConfig,
    pub chunking: ChunkingConfig,
    pub retrieval: RetrievalConfig,
    pub embedding: EmbeddingConfig,
    pub server: ServerConfig,
    pub connectors: ConnectorsConfig,
    /// Tool script configurations.
    #[serde(default)]
    pub tools: ToolsConfig,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ToolsConfig {
    /// Named Lua tool scripts.
    #[serde(default)]
    pub script: HashMap<String, ScriptToolConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ScriptToolConfig {
    /// Path to the .lua tool script.
    pub path: PathBuf,
    /// Maximum execution time in seconds. Default: 30.
    #[serde(default = "default_tool_timeout")]
    pub timeout: u64,
    /// All other config keys — accessible via context.config in the script.
    #[serde(flatten)]
    pub extra: toml::Table,
}

fn default_tool_timeout() -> u64 {
    30
}
```

### 7.3 Server Changes

```rust
// In server.rs — update run_server()

pub async fn run_server(config: &Config) -> anyhow::Result<()> {
    // Load tool definitions at startup
    let tool_defs = tool_script::load_tool_definitions(config)?;

    println!("Loaded {} Lua tool(s):", tool_defs.len());
    for t in &tool_defs {
        println!("  POST /tools/{} — {}", t.name, t.description);
    }

    let state = AppState {
        config: Arc::new(config.clone()),
        tools: Arc::new(tool_defs),
    };

    let mut app = Router::new()
        .route("/tools/search", post(handle_search))
        .route("/tools/get", post(handle_get))
        .route("/tools/sources", get(handle_sources))
        .route("/tools/list", get(handle_list_tools))   // NEW
        .route("/tools/{name}", post(handle_tool_call))  // NEW: dynamic
        .route("/health", get(handle_health))
        .layer(cors)
        .with_state(state);

    // ...
}
```

### 7.4 Context Bridge Implementation

The context bridge requires calling async Rust functions from the
synchronous Lua thread. This uses `tokio::runtime::Handle::block_on`:

```rust
fn register_context_bridge(lua: &Lua, config: &Config) -> LuaResult<()> {
    let ctx = lua.create_table()?;

    // context.search(query, opts?) → results
    let cfg = config.clone();
    ctx.set("search", lua.create_function(move |lua, (query, opts): (String, Option<LuaTable>)| {
        let mode = opts.as_ref()
            .and_then(|o| o.get::<String>("mode").ok())
            .unwrap_or_else(|| "keyword".to_string());
        let limit = opts.as_ref()
            .and_then(|o| o.get::<i64>("limit").ok())
            .unwrap_or(12);
        let source = opts.as_ref()
            .and_then(|o| o.get::<String>("source").ok());

        // Block on the async search function
        let handle = tokio::runtime::Handle::current();
        let results = handle.block_on(async {
            search_documents(&cfg, &query, &mode, source.as_deref(), None, Some(limit)).await
        }).map_err(|e| mlua::Error::external(e))?;

        // Convert results to Lua table
        results_to_lua_table(lua, &results)
    })?)?;

    // context.get(id) → document
    let cfg = config.clone();
    ctx.set("get", lua.create_function(move |lua, id: String| {
        let handle = tokio::runtime::Handle::current();
        let doc = handle.block_on(async {
            get_document(&cfg, &id).await
        }).map_err(|e| mlua::Error::external(e))?;

        doc_to_lua_table(lua, &doc)
    })?)?;

    // context.sources() → sources
    let cfg = config.clone();
    ctx.set("sources", lua.create_function(move |lua, ()| {
        let sources = get_sources(&cfg);
        sources_to_lua_table(lua, &sources)
    })?)?;

    lua.globals().set("context", ctx)?;
    Ok(())
}
```

### 7.5 Parameter Validation

Before calling `tool.execute()`, the server validates the incoming JSON
parameters against the tool's schema:

1. **Required fields** — return 400 if missing
2. **Type checking** — return 400 if wrong JSON type
3. **Enum validation** — return 400 if value not in allowed set
4. **Default injection** — fill in defaults for missing optional fields

This validation happens in Rust before the Lua VM is even created,
ensuring fast failure and clear error messages.

### 7.6 CLI Changes

```rust
// In main.rs — add new subcommand

/// Manage Lua tool scripts.
Tool {
    #[command(subcommand)]
    action: ToolAction,
},

#[derive(Subcommand)]
enum ToolAction {
    /// Scaffold a new tool from a template.
    Init { name: String },
    /// Test a tool script with sample parameters.
    Test {
        path: PathBuf,
        /// Tool parameters as key=value pairs.
        #[arg(long = "param", value_parser = parse_key_val)]
        params: Vec<(String, String)>,
        /// Named source from config for tool settings.
        #[arg(long)]
        source: Option<String>,
    },
    /// List all configured tools and their parameters.
    List,
}
```

### 7.7 Shared Lua Runtime Code

The following functions from `connector_script.rs` are reused by
`tool_script.rs` (extracted to a shared `lua_runtime.rs` module):

| Function | Purpose |
|----------|---------|
| `sandbox_globals()` | Remove dangerous Lua stdlib |
| `register_http_api()` | HTTP client |
| `register_json_api()` | JSON encode/decode |
| `register_env_api()` | Environment variable access |
| `register_log_api()` | Structured logging |
| `register_fs_api()` | Sandboxed filesystem |
| `register_base64_api()` | Base64 encode/decode |
| `register_crypto_api()` | SHA-256 and HMAC |
| `register_sleep()` | Sleep/backoff |
| `toml_table_to_lua()` | Config conversion |
| `expand_env_vars()` | `${VAR}` expansion |
| `json_value_to_lua()` | JSON → Lua conversion |
| `lua_value_to_json()` | Lua → JSON conversion |

This refactor moves the reusable runtime into `lua_runtime.rs`, with
`connector_script.rs` and `tool_script.rs` both importing from it.

---

## 8. HTTP API Additions

### 8.1 `GET /tools/list` — Tool Discovery

Returns all registered tools (built-in and Lua) with their OpenAI
function-calling schemas.

**Response Schema:**

```json
{
  "tools": [
    {
      "name": "string",
      "description": "string",
      "builtin": true,
      "parameters": {
        "type": "object",
        "properties": { ... },
        "required": ["..."]
      }
    }
  ]
}
```

Built-in tools (`search`, `get`, `sources`) are always included with
`"builtin": true`. Lua tools have `"builtin": false`.

### 8.2 `POST /tools/{name}` — Dynamic Tool Call

Calls a registered Lua tool. The request body is the tool's parameters
as a flat JSON object.

**Request:** tool parameters as JSON object.

**Response (success, 200):**

```json
{
  "result": { ... }
}
```

**Response (parameter validation error, 400):**

```json
{
  "error": {
    "code": "bad_request",
    "message": "missing required parameter: title"
  }
}
```

**Response (tool not found, 404):**

```json
{
  "error": {
    "code": "not_found",
    "message": "no tool registered with name: foo"
  }
}
```

**Response (script error, 500):**

```json
{
  "error": {
    "code": "tool_error",
    "message": "tools/create-jira.lua:42: attempt to index nil value"
  }
}
```

**Response (timeout, 408):**

```json
{
  "error": {
    "code": "timeout",
    "message": "tool 'create_jira_ticket' timed out after 30 seconds"
  }
}
```

---

## 9. Security & Sandboxing

### 9.1 Same Sandbox as Connectors

Tool scripts run in the same sandboxed Lua VM as connector scripts:

- No `os`, `io`, `debug`, `loadfile`, `dofile`
- Filesystem access sandboxed to script directory
- HTTP via host `reqwest` client (TLS enforced, proxy respected)
- Instruction-count timeout hook

### 9.2 Tool-Specific Concerns

Unlike connectors (which are batch processes run by the operator), tools
are invoked by AI agents in real-time. Additional considerations:

| Concern | Mitigation |
|---------|------------|
| Unauthorized tool calls | Server is local-only by default (`127.0.0.1`) |
| Rapid-fire invocations | Configurable per-tool rate limit (future) |
| Sensitive config exposure | Config values NOT included in `/tools/list` response |
| Script errors leaking internals | Error messages are sanitized (no stack traces in production) |

### 9.3 Future: Approval Mode

A future version may support an `approve` flag:

```toml
[tools.script.deploy_service]
path = "tools/deploy.lua"
approve = true  # requires human confirmation before execution
```

When `approve = true`, the server would return a `202 Accepted` with a
confirmation token, and the agent would need to present it to the user
for approval before re-submitting with the token.

---

## 10. Testing Strategy

### 10.1 Unit Tests (Rust)

- `test_lua_params_to_json_schema` — verify parameter conversion
- `test_parameter_validation` — required fields, types, enums, defaults
- `test_context_bridge_search` — verify context.search() calls through
- `test_context_bridge_get` — verify context.get() calls through
- `test_tool_timeout` — verify timeout enforcement
- `test_tool_error_handling` — verify Lua errors become proper HTTP responses

### 10.2 Integration Tests

- `test_tool_list_endpoint` — verify `/tools/list` returns built-in + Lua tools
- `test_tool_call_endpoint` — call a test tool via HTTP, verify response
- `test_tool_validation_errors` — verify 400 responses for bad params
- `test_tool_not_found` — verify 404 for unknown tool names
- `test_tool_context_search` — tool uses context.search(), verify results
- `test_tool_full_round_trip` — ingest → search via tool → act on results

### 10.3 `ctx tool test`

Runs a tool script locally with provided parameters:

```bash
$ ctx tool test tools/create-jira-ticket.lua \
    --param title="Test ticket" \
    --param body="Testing the tool" \
    --source create_jira_ticket

Testing tool: create_jira_ticket (tools/create-jira-ticket.lua)
  ✓ Script loaded
  ✓ tool.execute defined
  ✓ 5 parameters declared
  ✓ Calling execute({title: "Test ticket", body: "Testing the tool"})...
  ✓ Returned result in 1.2s

Result:
  {
    "success": true,
    "ticket_key": "ENG-1234",
    "url": "https://mycompany.atlassian.net/browse/ENG-1234",
    "message": "Created ENG-1234: Test ticket"
  }
```

---

## 11. Browser / Agent Integration

### 11.1 Browser Chat Agent

The browser chat on the docs page already supports tool calling. With
Lua tools registered, the agent can discover and call them:

1. Browser calls `GET /tools/list` → gets all tool schemas
2. Schemas are injected into the LLM's function-calling system message
3. LLM decides to call a tool → browser calls `POST /tools/{name}`
4. Result is fed back to the LLM for summarization

### 11.2 Cursor MCP Integration

Cursor's MCP protocol already supports tool discovery. The `/tools/list`
endpoint provides exactly the schema format Cursor expects. No adapter
needed.

### 11.3 Claude Desktop / Other Agents

Any MCP-compatible client can:
1. `GET /tools/list` to discover tools
2. `POST /tools/{name}` to call them
3. Parse the standard JSON response

---

## 12. Migration Path

### Phase 1: Core Runtime (this spec)

- Add `ToolsConfig` and `ScriptToolConfig` to config
- Extract shared Lua runtime to `lua_runtime.rs`
- Implement `tool_script.rs` (load, validate, execute)
- Add `GET /tools/list` endpoint
- Add `POST /tools/{name}` dynamic endpoint
- Add `ctx tool init`, `ctx tool test`, `ctx tool list` CLI commands
- Ship 1-2 example tools
- Update DESIGN.md, SCHEMAS.md, CHANGELOG.md

### Phase 2: Polish

- Parameter validation with detailed error messages
- Rate limiting per tool
- `ctx tool install` (community registry, shared with connectors)
- Hot-reload: watch tool scripts for changes, re-register routes

### Phase 3: Advanced

- Approval mode for destructive tools
- Tool chaining: one tool's output feeds another's input
- Tool observability: execution log with timing, params, results
- Streaming results (SSE) for long-running tools

---

## 13. Relationship to Lua Connectors

| Aspect | Connectors | Tools |
|--------|-----------|-------|
| Direction | External → Knowledge Base | Knowledge Base → External |
| Trigger | `ctx sync` (batch, operator) | `POST /tools/{name}` (real-time, agent) |
| Returns | `Vec<SourceItem>` | Arbitrary JSON |
| Context bridge | No | Yes (`context.search`, etc.) |
| Host APIs | Yes (http, json, etc.) | Yes (same + context) |
| Timeout default | 300s | 30s |
| Config location | `[connectors.script.*]` | `[tools.script.*]` |
| Shared runtime | `lua_runtime.rs` | `lua_runtime.rs` |

The two systems share the same Lua VM setup, host APIs, and sandboxing.
The only differences are the script interface (`connector.scan` vs
`tool.execute`), the context bridge, and where they're wired in.

---

## 14. File Structure After Implementation

```
src/
├── lua_runtime.rs          # NEW: shared Lua VM setup + host APIs
├── connector_script.rs     # Refactored: imports from lua_runtime
├── tool_script.rs          # NEW: tool loading, validation, execution
├── server.rs               # Updated: /tools/list, /tools/{name}
├── config.rs               # Updated: ToolsConfig, ScriptToolConfig
├── main.rs                 # Updated: ctx tool init/test/list
└── ...

tools/                      # Convention: tool scripts live here
├── create-jira-ticket.lua
├── post-slack.lua
└── ...

examples/
├── connectors/
│   └── github-issues.lua
└── tools/                  # NEW: example tool scripts
    └── echo.lua            # Minimal tool for testing
```

---

## 15. Stability

This document defines the public contract for Lua tool scripts. If the
tool interface, parameter schema format, context bridge API, or HTTP
endpoints change, this document must be updated.

The public contract is defined by:
- `LUA_TOOLS.md` (this document)
- `LUA_CONNECTORS.md`
- `SCHEMAS.md`
- `DESIGN.md`

