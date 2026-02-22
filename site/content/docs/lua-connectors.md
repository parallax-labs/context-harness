+++
title = "Lua Scripted Connectors"
description = "Write custom data source connectors in Lua without compiling Rust."
weight = 5

[extra]
sidebar_label = "Lua Connectors"
sidebar_group = "Extensibility"
sidebar_order = 5
+++

Lua scripted connectors let you add custom data sources to Context Harness by writing a simple Lua script. No Rust compilation needed — scripts run in a sandboxed Lua 5.4 VM with access to HTTP, JSON, filesystem, and other host APIs.

## How It Works

1. Write a `.lua` file that implements `connector.scan(config) → items[]`
2. Configure it in `ctx.toml` under `[connectors.script.<name>]`
3. Run `ctx sync script:<name>` to ingest

## Configuration

```toml
[connectors.script.jira]
path = "connectors/jira.lua"
timeout = 30
url = "https://mycompany.atlassian.net"
project = "ENG"
api_token = "${JIRA_API_TOKEN}"
```

- `path` — path to the `.lua` script
- `timeout` — execution timeout in seconds (default: 30)
- All other keys become the `config` table passed to `connector.scan()`
- Values support `${VAR_NAME}` environment variable expansion

## Script Contract

Every connector script must define a `connector` table with metadata and a `scan()` function:

```lua
connector = {
    name = "jira",
    version = "0.1.0",
    description = "Ingest Jira issues",
}

function connector.scan(config)
    -- config contains all keys from ctx.toml (url, project, api_token, etc.)
    local items = {}

    -- Fetch data using the http host API
    local resp = http.get(config.url .. "/rest/api/3/search", {
        headers = {
            ["Authorization"] = "Basic " .. base64.encode("user:" .. config.api_token),
        },
    })
    local data = json.decode(resp.body)

    for _, issue in ipairs(data.issues) do
        table.insert(items, {
            source_id  = issue.key,
            title      = issue.fields.summary,
            body       = issue.fields.description or "",
            author     = issue.fields.reporter.displayName,
            created_at = issue.fields.created,
            updated_at = issue.fields.updated,
            source_url = config.url .. "/browse/" .. issue.key,
            metadata   = { status = issue.fields.status.name },
        })
    end

    return items
end
```

### Return Schema

Each item in the returned array must have:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `source_id` | string | **yes** | Unique identifier within this source |
| `body` | string | **yes** | Document content |
| `title` | string | no | Document title |
| `author` | string | no | Author name |
| `created_at` | string | no | ISO 8601 timestamp |
| `updated_at` | string | no | ISO 8601 timestamp |
| `source_url` | string | no | Web URL for viewing |
| `content_type` | string | no | MIME type (default: `text/plain`) |
| `metadata` | table | no | Arbitrary key-value metadata |

## Host APIs

Scripts have access to these sandboxed APIs:

### `http` — HTTP Client

```lua
local resp = http.get(url, { headers = { ... } })
local resp = http.post(url, body, { headers = { ... } })
local resp = http.put(url, body, { headers = { ... } })
-- resp.status, resp.body, resp.headers
```

### `json` — JSON Encoding

```lua
local data = json.decode('{"key": "value"}')
local str  = json.encode({ key = "value" })
```

### `env` — Environment Variables

```lua
local key = env.get("API_KEY")
```

### `log` — Logging

```lua
log.info("Processing item: " .. id)
log.warn("Missing field")
log.error("API call failed")
log.debug("Debug details")
```

### `fs` — Sandboxed File Access

```lua
local content = fs.read("path/to/file.txt")
local files   = fs.list("directory/")
```

### `base64` — Encoding

```lua
local encoded = base64.encode("hello")
local decoded = base64.decode(encoded)
```

### `crypto` — Hashing

```lua
local hash = crypto.sha256("data")
local hmac = crypto.hmac_sha256("key", "data")
```

### `sleep` — Delay

```lua
sleep(1.5)  -- sleep 1.5 seconds
```

## CLI Commands

### Scaffold a New Connector

```bash
$ ctx connector init my-connector
# Created connectors/my-connector.lua
```

### Test a Connector

```bash
$ ctx connector test connectors/my-connector.lua
```

### Sync a Connector

```bash
$ ctx sync script:my-connector --config ./config/ctx.toml
```

## Example: GitHub Issues

See [`examples/connectors/github-issues.lua`](https://github.com/parallax-labs/context-harness/blob/main/examples/connectors/github-issues.lua) for a complete example that fetches GitHub issues using the REST API.

