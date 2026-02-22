# Lua Scripted Connectors — Design Specification

This document specifies the design for runtime-loadable Lua connectors in
Context Harness. Lua scripts implement the connector interface without
requiring Rust compilation, enabling users and AI agents to add new data
sources dynamically.

**Status:** Proposed  
**Author:** Parker Jones  
**Created:** 2026-02-21

---

## 1. Motivation

Context Harness ships three compiled connectors: filesystem, Git, and S3.
Adding a new connector today requires writing Rust, compiling, and releasing
a new binary. This creates friction for:

- **Users** who need to ingest from team-specific sources (Jira, Confluence,
  Notion, internal APIs, custom databases)
- **AI agents** (Cursor, Claude) that could scaffold and test connectors
  on-the-fly without leaving the IDE
- **Community contributions** that shouldn't require deep Rust knowledge

The connector interface is a natural scripting boundary: the contract is
simply "take configuration, return a list of documents." Everything
downstream (chunking, embedding, indexing, search) stays in Rust.

### Why Lua

| Criterion | Lua | WASM | Python | Rhai | JS/V8 |
|-----------|-----|------|--------|------|-------|
| Runtime size | ~200 KB | ~2 MB | ~30 MB | ~500 KB | ~20 MB |
| Embedding maturity | Excellent (`mlua`) | Good | Complex | Good | Complex |
| LLM generation quality | High | Low | High | Low | High |
| Sandboxing | Good (remove stdlib) | Excellent | Poor | Good | Good |
| Async support | Via `mlua` async | Native | N/A | No | Yes |
| Developer familiarity | Medium | Low | High | Low | High |
| Battle-tested embedding | Redis, Nginx, games | Emerging | Rare | Niche | Node |

Lua wins on the combination of: tiny runtime, mature Rust bindings (`mlua`
supports Lua 5.4 and LuaJIT), LLMs generate valid Lua reliably, and the
embedding pattern is battle-tested (Redis EVAL, OpenResty, game scripting).

---

## 2. User-Facing API

### 2.1 Configuration

Script connectors are configured under `[connectors.script.<name>]`. The
`path` key points to a `.lua` file. All other keys become the `config`
table passed to the script's `scan()` function.

```toml
[connectors.script.jira]
path = "connectors/jira.lua"
url = "https://mycompany.atlassian.net"
api_token = "${JIRA_API_TOKEN}"
project_key = "ENG"

[connectors.script.confluence]
path = "connectors/confluence.lua"
url = "https://mycompany.atlassian.net/wiki"
space_key = "ENG"
api_token = "${CONFLUENCE_API_TOKEN}"
```

**Environment variable expansion:** Values containing `${VAR_NAME}` are
expanded from the process environment before being passed to Lua. This
keeps secrets out of config files.

### 2.2 CLI

```bash
# Sync a specific script connector
ctx sync script --source jira --config ctx.toml

# Sync all script connectors
ctx sync script --config ctx.toml

# Dry-run to see what a connector would ingest
ctx sync script --source jira --dry-run --config ctx.toml

# Test a connector script without writing to the database
ctx connector test connectors/jira.lua --config ctx.toml

# Scaffold a new connector from a template
ctx connector init my-connector
```

When `--source` is omitted, `ctx sync script` iterates all entries under
`[connectors.script.*]` and syncs each one sequentially.

### 2.3 Source Naming

Documents produced by script connectors use the source format
`"script:<name>"` (e.g., `"script:jira"`). This enables filtering:

```bash
ctx search "auth migration" --source script:jira
```

---

## 3. Lua Script Interface

### 3.1 Contract

Every connector script MUST define a global `connector` table with at
minimum a `scan` function:

```lua
-- Required: connector metadata
connector = {
    name = "jira",          -- identifier (matches config key)
    version = "1.0",        -- semver string
    description = "Ingest Jira issues and comments",  -- optional
}

-- Required: scan function
-- Arguments:
--   config (table) — all key-value pairs from the TOML config section
--                    (excluding 'path')
-- Returns:
--   items (table) — array of source item tables
function connector.scan(config)
    -- ... fetch data, transform, return items
    return items
end
```

### 3.2 Source Item Schema

Each item in the returned array is a Lua table with the following fields:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `source` | string | no | Override source name (default: `"script:<name>"`) |
| `source_id` | string | **yes** | Unique ID within this source |
| `title` | string | no | Human-readable title |
| `body` | string | **yes** | Full text content |
| `content_type` | string | no | MIME type (default: `"text/plain"`) |
| `source_url` | string | no | Web URL to the original item |
| `author` | string | no | Author/creator name |
| `created_at` | string or number | no | ISO 8601 string or Unix timestamp |
| `updated_at` | string or number | no | ISO 8601 string or Unix timestamp |
| `metadata_json` | string | no | Arbitrary JSON metadata |

Example:

```lua
{
    source_id = "ENG-1234",
    title = "Implement auth token refresh",
    body = "As a user, I want my auth tokens to refresh automatically...",
    content_type = "text/plain",
    source_url = "https://mycompany.atlassian.net/browse/ENG-1234",
    author = "Jane Smith",
    updated_at = "2026-02-20T14:30:00Z",
    metadata_json = '{"priority":"high","status":"In Progress"}',
}
```

### 3.3 Validation

The Rust host validates each returned item:

1. `source_id` must be a non-empty string
2. `body` must be a non-empty string
3. `updated_at` / `created_at` are parsed as ISO 8601 or Unix timestamps;
   if missing or invalid, the current time is used
4. `content_type` defaults to `"text/plain"` if absent
5. Items failing validation are logged as warnings and skipped (not fatal)

---

## 4. Host-Provided APIs

The Lua VM is initialized with the following modules available as globals.
These are implemented in Rust and exposed to Lua via `mlua`.

### 4.1 `http` — HTTP Client

```lua
-- GET request
local resp = http.get(url, {
    headers = { ["Authorization"] = "Bearer " .. token },
    params = { page = "1", limit = "50" },  -- query parameters
    timeout = 30,  -- seconds (default: 30)
})

-- POST request
local resp = http.post(url, body_string, {
    headers = { ["Content-Type"] = "application/json" },
})

-- PUT request
local resp = http.put(url, body_string, {
    headers = { ["Content-Type"] = "application/json" },
})
```

Response table:

```lua
{
    status = 200,           -- HTTP status code
    headers = { ... },      -- response headers (lowercase keys)
    body = "...",           -- raw response body as string
    json = { ... },         -- parsed JSON (nil if not valid JSON)
    ok = true,              -- true if status is 2xx
}
```

**Implementation:** Uses the host process's `reqwest` client. Follows
redirects. Respects system proxy settings. Connection pooled.

### 4.2 `json` — JSON Encoding/Decoding

```lua
local table = json.parse('{"key": "value"}')
local str = json.encode({ key = "value" })
```

### 4.3 `env` — Environment Variables

```lua
local api_key = env.get("JIRA_API_TOKEN")  -- returns string or nil
```

### 4.4 `log` — Structured Logging

```lua
log.info("Fetching page 3 of issues")
log.warn("Rate limited, backing off 2s")
log.error("Failed to parse response: " .. err)
log.debug("Raw response: " .. resp.body)  -- only shown with --verbose
```

Log output includes the connector name as context:
`[script:jira] INFO: Fetching page 3 of issues`

### 4.5 `fs` — Sandboxed File Access

```lua
-- Read a file (path relative to connector script location)
local content = fs.read("./templates/issue.md")

-- List files matching a glob
local files = fs.list("./data", "*.csv")
-- files = { { path = "./data/export.csv", size = 1234, modified = 1708531200 }, ... }
```

**Sandboxing:** `fs.read` and `fs.list` are restricted to the directory
containing the connector script and its subdirectories. Attempting to
access paths outside this sandbox (e.g., `../../etc/passwd`) returns an
error.

### 4.6 `base64` — Base64 Encoding/Decoding

```lua
local encoded = base64.encode("hello")   -- "aGVsbG8="
local decoded = base64.decode(encoded)   -- "hello"
```

### 4.7 `crypto` — Hashing and HMAC

```lua
local hash = crypto.sha256("data")               -- hex string
local sig = crypto.hmac_sha256("key", "data")    -- hex string
```

### 4.8 `sleep` — Rate Limit Backoff

```lua
sleep(2)  -- pause execution for 2 seconds
```

**Implementation:** Maps to `tokio::time::sleep` on the async runtime.
Does not block other connectors if running in parallel.

---

## 5. Complete Example: Jira Connector

```lua
connector = {
    name = "jira",
    version = "1.0",
    description = "Ingest Jira issues with comments",
}

function connector.scan(config)
    local items = {}
    local start_at = 0
    local page_size = 50

    -- Paginate through all issues
    while true do
        log.info("Fetching issues starting at " .. start_at)

        local resp = http.get(config.url .. "/rest/api/2/search", {
            headers = {
                ["Authorization"] = "Basic " .. base64.encode(
                    config.email .. ":" .. config.api_token
                ),
                ["Accept"] = "application/json",
            },
            params = {
                jql = "project = " .. config.project_key
                    .. " ORDER BY updated DESC",
                startAt = tostring(start_at),
                maxResults = tostring(page_size),
                fields = "summary,description,comment,updated,creator,status",
            },
        })

        if not resp.ok then
            log.error("Jira API error: " .. resp.status .. " " .. resp.body)
            break
        end

        local data = resp.json
        if not data or not data.issues then break end

        for _, issue in ipairs(data.issues) do
            -- Build body from description + comments
            local body = issue.fields.description or ""

            if issue.fields.comment and issue.fields.comment.comments then
                for _, comment in ipairs(issue.fields.comment.comments) do
                    body = body .. "\n\n---\n\n"
                    body = body .. "**" .. (comment.author.displayName or "Unknown")
                    body = body .. "** (" .. comment.created .. "):\n"
                    body = body .. (comment.body or "")
                end
            end

            table.insert(items, {
                source_id = issue.key,
                title = issue.key .. ": " .. (issue.fields.summary or ""),
                body = body,
                content_type = "text/plain",
                source_url = config.url .. "/browse/" .. issue.key,
                author = issue.fields.creator
                    and issue.fields.creator.displayName or nil,
                updated_at = issue.fields.updated,
                metadata_json = json.encode({
                    status = issue.fields.status
                        and issue.fields.status.name or nil,
                    project = config.project_key,
                }),
            })
        end

        -- Check if there are more pages
        start_at = start_at + page_size
        if start_at >= data.total then break end

        -- Be nice to the API
        sleep(0.5)
    end

    log.info("Fetched " .. #items .. " issues from " .. config.project_key)
    return items
end
```

---

## 6. Rust Implementation

### 6.1 New Crate Dependency

```toml
[dependencies]
mlua = { version = "0.10", features = ["lua54", "async", "serialize"] }
```

### 6.2 New Module: `connector_script.rs`

Responsible for:

1. Loading the Lua script file
2. Creating a sandboxed Lua VM with host APIs
3. Converting TOML config to a Lua table
4. Calling `connector.scan(config)`
5. Converting the returned Lua table to `Vec<SourceItem>`
6. Validating items and logging warnings for invalid entries

```rust
// Pseudocode — actual implementation will follow this structure

pub async fn scan_script(
    config: &Config,
    name: &str,
    script_config: &ScriptConnectorConfig,
) -> Result<Vec<SourceItem>> {
    // 1. Read the Lua script
    let script_src = std::fs::read_to_string(&script_config.path)?;

    // 2. Create Lua VM
    let lua = Lua::new();
    lua.sandbox(true)?;

    // 3. Register host APIs
    register_http_api(&lua)?;
    register_json_api(&lua)?;
    register_env_api(&lua)?;
    register_log_api(&lua, name)?;
    register_fs_api(&lua, &script_config.path)?;
    register_base64_api(&lua)?;
    register_crypto_api(&lua)?;
    register_sleep_api(&lua)?;

    // 4. Execute the script (defines connector.scan)
    lua.load(&script_src).exec()?;

    // 5. Build the config table from TOML
    let config_table = toml_to_lua_table(&lua, &script_config.extra)?;

    // 6. Call connector.scan(config)
    let connector: Table = lua.globals().get("connector")?;
    let scan: Function = connector.get("scan")?;
    let result: Table = scan.call(config_table)?;

    // 7. Convert Lua table -> Vec<SourceItem>
    let items = lua_table_to_source_items(result, name)?;

    Ok(items)
}
```

### 6.3 Config Changes

```rust
// In config.rs

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ConnectorsConfig {
    pub filesystem: Option<FilesystemConnectorConfig>,
    pub git: Option<GitConnectorConfig>,
    pub s3: Option<S3ConnectorConfig>,
    /// Named Lua script connectors.
    /// Key is the connector name, value is the script config.
    #[serde(default)]
    pub script: HashMap<String, ScriptConnectorConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ScriptConnectorConfig {
    /// Path to the .lua connector script.
    pub path: PathBuf,
    /// All other keys are passed as config to the Lua scan() function.
    #[serde(flatten)]
    pub extra: toml::Table,
}
```

### 6.4 Ingest Pipeline Changes

```rust
// In ingest.rs — update the match block in run_sync()

let mut items = match connector {
    "filesystem" => connector_fs::scan_filesystem(config)?,
    "git" => connector_git::scan_git(config)?,
    "s3" => connector_s3::scan_s3(config).await?,
    c if c.starts_with("script:") => {
        let name = &c[7..];
        let script_cfg = config.connectors.script.get(name)
            .ok_or_else(|| anyhow!("No script connector configured: '{}'", name))?;
        connector_script::scan_script(config, name, script_cfg).await?
    }
    _ => bail!("Unknown connector: '{}'. Available: filesystem, git, s3, script:<name>", connector),
};
```

The CLI `ctx sync script --source jira` maps to connector name `"script:jira"`.
When `--source` is omitted, iterate all keys in `config.connectors.script`.

### 6.5 CLI Changes

```rust
// In main.rs — add new subcommand

/// Manage Lua connector scripts.
Connector {
    #[command(subcommand)]
    action: ConnectorAction,
},

#[derive(Subcommand)]
enum ConnectorAction {
    /// Test a connector script without writing to the database.
    Test {
        /// Path to the .lua connector script.
        path: PathBuf,
        /// Named source from config to use for settings.
        #[arg(long)]
        source: Option<String>,
    },
    /// Scaffold a new connector from a template.
    Init {
        /// Name for the new connector.
        name: String,
    },
}
```

---

## 7. Error Handling

### 7.1 Script Errors

Lua runtime errors (syntax errors, nil access, type mismatches) are caught
by `mlua` and converted to `anyhow::Error` with the Lua stack trace
included:

```
Error: script connector 'jira' failed:
  connectors/jira.lua:42: attempt to index a nil value (field 'fields')
  stack traceback:
    connectors/jira.lua:42: in function 'scan'
```

### 7.2 Item Validation Errors

Invalid items are logged as warnings and skipped. The sync continues:

```
[script:jira] WARN: Skipping item at index 5: missing required field 'source_id'
[script:jira] WARN: Skipping item at index 12: 'body' is empty
```

### 7.3 HTTP Errors

HTTP failures are returned to the Lua script as non-ok responses (not
exceptions). The script decides how to handle them:

```lua
local resp = http.get(url, opts)
if not resp.ok then
    if resp.status == 429 then
        log.warn("Rate limited, sleeping 5s")
        sleep(5)
        -- retry...
    else
        log.error("API error: " .. resp.status)
        return items  -- return what we have so far
    end
end
```

### 7.4 Timeout

A configurable timeout (default: 300 seconds) limits total script
execution time. If exceeded, the script is terminated and the error is
reported:

```
Error: script connector 'jira' timed out after 300 seconds
```

Configuration:

```toml
[connectors.script.jira]
path = "connectors/jira.lua"
timeout = 600  # seconds (default: 300)
```

---

## 8. Security & Sandboxing

### 8.1 Restricted Standard Library

The Lua VM is initialized with these standard libraries **removed**:

- `os` — no OS-level access (process, clock, env via `os.getenv`)
- `io` — no raw file I/O (use `fs.*` instead)
- `loadfile` / `dofile` — no loading external Lua files
- `debug` — no debug introspection

### 8.2 Filesystem Sandbox

`fs.read()` and `fs.list()` are restricted to the directory containing
the connector script. Path traversal attempts are rejected:

```lua
fs.read("../../etc/passwd")  -- Error: path escapes sandbox
```

### 8.3 Network Access

HTTP requests go through the host's `reqwest` client. This means:

- System proxy settings are respected
- TLS verification is enforced
- Connection pooling across requests
- No raw socket access from Lua

### 8.4 Resource Limits

| Resource | Default Limit | Configurable |
|----------|---------------|-------------|
| Execution time | 300 seconds | `timeout` in config |
| Memory | 256 MB | `memory_limit` in config |
| HTTP requests | Unlimited | No (script handles rate limiting) |
| Response body size | 50 MB per request | `max_response_size` in config |

---

## 9. Connector Template

`ctx connector init my-connector` generates:

```
connectors/
└── my-connector.lua
```

Template content:

```lua
--[[
  Context Harness Connector: my-connector
  
  Configuration (add to ctx.toml):
  
    [connectors.script.my-connector]
    path = "connectors/my-connector.lua"
    url = "https://api.example.com"
    api_token = "${MY_CONNECTOR_API_TOKEN}"
  
  Sync:
    ctx sync script --source my-connector
  
  Test:
    ctx connector test connectors/my-connector.lua
]]

connector = {
    name = "my-connector",
    version = "0.1.0",
    description = "TODO: describe what this connector ingests",
}

--- Scan the data source and return a list of items to ingest.
--- @param config table Configuration from ctx.toml
--- @return table Array of source item tables
function connector.scan(config)
    local items = {}

    -- TODO: Implement your connector logic here
    --
    -- Example: fetch from an API
    -- local resp = http.get(config.url .. "/api/items", {
    --     headers = { ["Authorization"] = "Bearer " .. config.api_token },
    -- })
    -- if not resp.ok then
    --     log.error("API error: " .. resp.status)
    --     return items
    -- end
    -- for _, item in ipairs(resp.json.items) do
    --     table.insert(items, {
    --         source_id = item.id,
    --         title = item.title,
    --         body = item.content,
    --         source_url = config.url .. "/items/" .. item.id,
    --         updated_at = item.updated_at,
    --     })
    -- end

    log.info("Fetched " .. #items .. " items")
    return items
end
```

---

## 10. Testing Strategy

### 10.1 Unit Tests (Rust)

- `test_lua_vm_creation` — verify Lua VM initializes with all host APIs
- `test_toml_to_lua_table` — verify config conversion
- `test_lua_table_to_source_items` — verify return value conversion
- `test_item_validation` — verify invalid items are skipped
- `test_env_expansion` — verify `${VAR}` expansion in config values
- `test_filesystem_sandbox` — verify path traversal is blocked
- `test_script_timeout` — verify timeout enforcement

### 10.2 Integration Tests

- `test_scan_with_mock_http` — run a real Lua script against a mock HTTP
  server, verify the returned `SourceItem`s
- `test_full_sync_pipeline` — script connector → ingest → chunks → search

### 10.3 `ctx connector test`

Runs a script and prints the items as JSON without writing to the database:

```bash
$ ctx connector test connectors/jira.lua --source jira
Testing connector: jira (connectors/jira.lua)
  ✓ Script loaded
  ✓ connector.scan defined
  ✓ Calling scan()...
  ✓ Returned 47 items
  ✓ All items valid

Items (first 3):
  [0] ENG-1234: Implement auth token refresh (2026-02-20)
  [1] ENG-1233: Fix rate limiter regression (2026-02-19)
  [2] ENG-1230: Add observability to payment service (2026-02-18)
```

---

## 11. Future Extensions

### 11.1 Incremental Sync

A future version may support incremental sync by allowing the script to
receive and return a checkpoint:

```lua
function connector.scan(config, checkpoint)
    -- checkpoint is the value returned by the previous scan
    local since = checkpoint or "2020-01-01T00:00:00Z"
    -- ... fetch only items updated since checkpoint
    return items, new_checkpoint  -- return both items and new checkpoint
end
```

### 11.2 Connector Registry

A public GitHub repository of community connector scripts:

```
context-harness-connectors/
├── jira.lua
├── confluence.lua
├── notion.lua
├── slack.lua
├── linear.lua
├── github-issues.lua
├── discord.lua
└── README.md
```

Install:

```bash
ctx connector install jira
# Downloads jira.lua to ./connectors/ and prints config template
```

### 11.3 Parallel Execution

When syncing all script connectors (`ctx sync script` without `--source`),
scripts could run in parallel using separate Lua VMs on a Tokio task pool.

### 11.4 Watcher Mode

A future `ctx watch` command could re-run script connectors on a schedule:

```toml
[connectors.script.jira]
path = "connectors/jira.lua"
watch_interval = "5m"  # re-sync every 5 minutes
```

---

## 12. Migration Path

### Phase 1: Core Runtime (this spec)

- Add `mlua` dependency
- Implement `connector_script.rs` with host APIs
- Add `[connectors.script.*]` config support
- Wire into `ctx sync script`
- Add `ctx connector test` and `ctx connector init`
- Ship 1-2 example connectors (Jira, GitHub Issues)

### Phase 2: Polish

- Add `ctx connector install` (registry)
- Add incremental sync support
- Add parallel execution
- Ship more connectors (Confluence, Notion, Slack, Linear)

### Phase 3: Agent Integration

- Document patterns for AI agents to generate connectors
- Add `.cursorrules` template that instructs agents how to create connectors
- Show examples of "ask Cursor to add a Jira connector" workflow

