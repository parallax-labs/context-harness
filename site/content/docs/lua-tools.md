+++
title = "Lua MCP Tool Extensions"
description = "Define custom MCP tools in Lua that AI agents can discover and call."
weight = 6

[extra]
sidebar_label = "Lua Tools"
sidebar_group = "Extensibility"
sidebar_order = 6
+++

Lua tool extensions let you define custom MCP tools that AI agents (Cursor, Claude, browser LLMs) can discover via `GET /tools/list` and call via `POST /tools/{name}` — without recompiling Rust.

While **connectors** read data *into* the knowledge base, **tools** let agents *act on* that data. Together they form a full bidirectional bridge between AI agents and external systems.

## Use Cases

| Use Case | Connector (read) | Tool (write/act) |
|----------|-------------------|-------------------|
| Jira | Ingest issues → search | Create/update tickets |
| Slack | Ingest threads → search | Post messages |
| GitHub | Ingest issues/PRs → search | Create issues, post comments |
| Internal APIs | Ingest docs → search | Trigger deploys, run queries |

## Configuration

Tool scripts are configured under `[tools.script.<name>]`:

```toml
[tools.script.create_jira_ticket]
path = "tools/create-jira-ticket.lua"
timeout = 30
jira_url = "https://mycompany.atlassian.net"
jira_project = "ENG"
jira_token = "${JIRA_API_TOKEN}"
```

- `path` — path to the `.lua` script
- `timeout` — execution timeout in seconds (default: 30)
- All other keys become `context.config` inside the script

## Script Contract

Every tool script defines a `tool` table with metadata, parameters, and an `execute()` function:

```lua
tool = {
    name = "create_jira_ticket",
    version = "0.1.0",
    description = "Create a Jira ticket enriched with related context",
    parameters = {
        {
            name = "title",
            type = "string",
            required = true,
            description = "Ticket title / summary",
        },
        {
            name = "priority",
            type = "string",
            required = false,
            default = "Medium",
            enum = { "Low", "Medium", "High", "Critical" },
            description = "Ticket priority",
        },
    },
}

function tool.execute(params, context)
    -- Search the knowledge base for related context
    local results = context.search(params.title, { limit = 3 })

    -- Build enriched description
    local desc = "## Related Context\n\n"
    for _, r in ipairs(results) do
        desc = desc .. "- [" .. r.title .. "](" .. (r.source_url or "") .. ")\n"
    end

    -- Call external API
    local resp = http.post(context.config.jira_url .. "/rest/api/3/issue", json.encode({
        fields = {
            project = { key = context.config.jira_project },
            summary = params.title,
            description = desc,
            priority = { name = params.priority },
            issuetype = { name = "Task" },
        },
    }), {
        headers = {
            ["Authorization"] = "Basic " .. base64.encode("user:" .. context.config.jira_token),
            ["Content-Type"] = "application/json",
        },
    })

    local result = json.decode(resp.body)
    return {
        key = result.key,
        url = context.config.jira_url .. "/browse/" .. result.key,
        related_docs = #results,
    }
end
```

### Parameter Schema

Each parameter in the `parameters` array supports:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | **yes** | Parameter name |
| `type` | string | **yes** | `"string"`, `"number"`, `"boolean"`, or `"integer"` |
| `required` | boolean | no | Whether the parameter is mandatory |
| `description` | string | no | Shown to agents for better tool use |
| `default` | any | no | Default value if not provided |
| `enum` | array | no | Restrict to specific allowed values |

Parameters are converted to OpenAI function-calling JSON Schema format, making them compatible with any agent that supports function calling.

## Context Bridge

The `context` argument passed to `tool.execute()` provides:

### `context.search(query, opts?)` → results[]

Search the knowledge base. Options: `mode` (keyword/semantic/hybrid), `limit`, `source`.

```lua
local results = context.search("authentication flow", {
    mode = "hybrid",
    limit = 5,
    source = "filesystem",
})

for _, r in ipairs(results) do
    print(r.title, r.score, r.snippet)
end
```

### `context.get(id)` → document

Retrieve a full document by UUID.

```lua
local doc = context.get("a1b2c3d4-...")
print(doc.title, doc.body)
```

### `context.sources()` → sources[]

List configured connectors and their status.

```lua
local sources = context.sources()
for _, s in ipairs(sources) do
    print(s.name, s.configured, s.healthy)
end
```

### `context.config` → table

Tool-specific configuration from `ctx.toml` (with environment variables expanded).

## Host APIs

Tool scripts have access to the same host APIs as connectors: `http`, `json`, `env`, `log`, `fs`, `base64`, `crypto`, and `sleep`. See [Lua Connectors](@/docs/lua-connectors.md) for details.

## HTTP Endpoints

### `GET /tools/list` — Tool Discovery

Returns all registered tools (built-in + Lua) with their parameter schemas:

```json
{
  "tools": [
    {
      "name": "search",
      "description": "Search indexed documents",
      "builtin": true,
      "parameters": { "type": "object", "properties": { ... } }
    },
    {
      "name": "create_jira_ticket",
      "description": "Create a Jira ticket enriched with related context",
      "builtin": false,
      "parameters": { "type": "object", "properties": { ... } }
    }
  ]
}
```

### `POST /tools/{name}` — Tool Invocation

Call any registered tool with JSON parameters:

```bash
$ curl -X POST http://localhost:7331/tools/create_jira_ticket \
    -H "Content-Type: application/json" \
    -d '{"title": "Fix auth bug", "priority": "High"}'
```

**Response codes:**
- `200` — success, returns `{ "result": { ... } }`
- `400` — parameter validation failed
- `404` — unknown tool name
- `408` — script execution timed out
- `500` — script error

## CLI Commands

### Scaffold a New Tool

```bash
$ ctx tool init my-tool
# Created tools/my-tool.lua
```

### Test a Tool

```bash
$ ctx tool test tools/my-tool.lua --param title="Test ticket" --param priority=High
```

### List Configured Tools

```bash
$ ctx tool list --config ./config/ctx.toml
```

## Examples

- **Echo tool** — [`examples/tools/echo.lua`](https://github.com/parallax-labs/context-harness/blob/main/examples/tools/echo.lua) — minimal test tool
- **Jira tool** — [`examples/tools/create-jira-ticket.lua`](https://github.com/parallax-labs/context-harness/blob/main/examples/tools/create-jira-ticket.lua) — full RAG-enriched example

