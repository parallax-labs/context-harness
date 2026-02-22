+++
title = "Lua MCP Tool Extensions"
description = "Define custom MCP tools in Lua that AI agents can discover and call at runtime."
weight = 6

[extra]
sidebar_label = "Lua Tools"
sidebar_group = "Extensibility"
sidebar_order = 6
+++

While **connectors** read data *into* the knowledge base, **tools** let agents *act on* that data. Lua tool extensions define custom MCP tools that AI agents (Cursor, Claude, browser LLMs) can discover via `GET /tools/list` and invoke via `POST /tools/{name}` — without recompiling Rust.

### Connectors vs. Tools

| | Connector (read) | Tool (write/act) |
|---|---|---|
| **Jira** | Ingest issues → search | Create/update tickets |
| **Slack** | Ingest threads → search | Post messages |
| **GitHub** | Ingest issues/PRs → search | Create issues, post comments |
| **Deploy** | — | Trigger deploys, run health checks |

### Example: RAG-enriched Jira ticket creation

This tool searches the knowledge base for related context, then creates a Jira ticket with that context attached:

```lua
tool = {
    name = "create_jira_ticket",
    version = "0.1.0",
    description = "Create a Jira ticket enriched with related context from the knowledge base",
    parameters = {
        {
            name = "title",
            type = "string",
            required = true,
            description = "Ticket title / summary",
        },
        {
            name = "description",
            type = "string",
            required = false,
            description = "Additional description text",
        },
        {
            name = "priority",
            type = "string",
            required = false,
            default = "Medium",
            enum = { "Low", "Medium", "High", "Critical" },
            description = "Ticket priority level",
        },
    },
}

function tool.execute(params, context)
    -- 1. Search the knowledge base for related docs
    local results = context.search(params.title, { limit = 5, mode = "hybrid" })

    -- 2. Build enriched description with related context
    local desc = params.description or ""
    if #results > 0 then
        desc = desc .. "\n\n## Related Context\n\n"
        for _, r in ipairs(results) do
            desc = desc .. "- **" .. r.title .. "** (score: " ..
                   string.format("%.2f", r.score) .. ")\n"
            desc = desc .. "  " .. (r.snippet or "") .. "\n"
            if r.source_url then
                desc = desc .. "  [View source](" .. r.source_url .. ")\n"
            end
        end
    end

    -- 3. Create the Jira ticket via API
    local payload = json.encode({
        fields = {
            project = { key = context.config.jira_project },
            summary = params.title,
            description = desc,
            priority = { name = params.priority },
            issuetype = { name = "Task" },
        },
    })

    local resp = http.post(
        context.config.jira_url .. "/rest/api/3/issue",
        payload,
        {
            headers = {
                ["Authorization"] = "Basic " .. base64.encode(
                    "user:" .. context.config.jira_token
                ),
                ["Content-Type"] = "application/json",
            },
        }
    )

    local result = json.decode(resp.body)
    return {
        key = result.key,
        url = context.config.jira_url .. "/browse/" .. result.key,
        related_docs = #results,
        status = "created",
    }
end
```

### Configuration

```toml
[tools.script.create_jira_ticket]
path = "tools/create-jira-ticket.lua"
timeout = 30
jira_url = "https://mycompany.atlassian.net"
jira_project = "ENG"
jira_token = "${JIRA_API_TOKEN}"
```

### Parameter schema

Each parameter supports:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | **yes** | Parameter name |
| `type` | string | **yes** | `"string"`, `"number"`, `"boolean"`, or `"integer"` |
| `required` | boolean | no | Whether the agent must provide it |
| `description` | string | no | Shown to the agent for better tool use |
| `default` | any | no | Used if the agent doesn't provide a value |
| `enum` | array | no | Restrict to specific allowed values |

Parameters are converted to OpenAI function-calling JSON Schema format, making them compatible with any agent that supports function calling.

### Context bridge

The `context` argument in `tool.execute(params, context)` provides:

**`context.search(query, opts?)`** — Search the knowledge base. Returns an array of results with `title`, `score`, `snippet`, `source_url`, `source`, `source_id`.

```lua
local results = context.search("auth flow", {
    mode = "hybrid",   -- "keyword" | "semantic" | "hybrid"
    limit = 5,
    source = "git",    -- Filter by source name
})
```

**`context.get(id)`** — Retrieve a full document by UUID.

```lua
local doc = context.get("a1b2c3d4-...")
print(doc.title, doc.body, doc.source_url)
```

**`context.sources()`** — List all configured data sources and their status.

**`context.config`** — Tool-specific config from `ctx.toml` (env vars already expanded).

### HTTP endpoints

**`GET /tools/list`** — Discover all registered tools with their schemas:

```bash
$ curl -s localhost:7331/tools/list | jq '.tools[] | {name, description, builtin}'
{"name": "search", "description": "Search indexed documents", "builtin": true}
{"name": "get_document", "description": "Retrieve full document by ID", "builtin": true}
{"name": "list_sources", "description": "List configured data sources", "builtin": true}
{"name": "create_jira_ticket", "description": "Create a Jira ticket enriched with related context", "builtin": false}
```

**`POST /tools/{name}`** — Invoke a tool:

```bash
$ curl -X POST localhost:7331/tools/create_jira_ticket \
    -H "Content-Type: application/json" \
    -d '{"title": "Fix auth token refresh", "priority": "High"}'

{
  "result": {
    "key": "ENG-1234",
    "url": "https://mycompany.atlassian.net/browse/ENG-1234",
    "related_docs": 3,
    "status": "created"
  }
}
```

### CLI commands

```bash
# Scaffold a new tool:
$ ctx tool init my-tool
Created tools/my-tool.lua

# Test with sample params:
$ ctx tool test tools/echo.lua --param message="hello world"
{
  "echo": "Echo: hello world",
  "source_count": 2
}

# List all configured tools:
$ ctx tool list
Built-in tools:
  search           Search indexed documents
  get_document     Retrieve full document by ID
  list_sources     List configured data sources

Lua tools:
  create_jira_ticket   Create a Jira ticket enriched with related context
  echo                 Echoes back the input message
```
