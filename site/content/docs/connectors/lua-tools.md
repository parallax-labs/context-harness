+++
title = "Lua Tools"
description = "Define custom MCP tools in Lua that AI agents can discover and call at runtime."
weight = 3
+++

While **connectors** read data *into* the knowledge base, **tools** let agents *act on* that data. Lua tool extensions define custom MCP tools that AI agents (Cursor, Claude, browser LLMs) can discover via `GET /tools/list` and invoke via `POST /tools/{name}` — without recompiling Rust.

### Connectors vs. Tools

| | Connector (read) | Tool (write/act) |
|---|---|---|
| **Jira** | Ingest issues → search | Create/update tickets |
| **Slack** | Ingest threads → search | Post messages |
| **GitHub** | Ingest issues/PRs → search | Create issues, post comments |
| **Deploy** | — | Trigger deploys, run health checks |
| **Docs** | — | Generate summaries, create ADRs |

### Simple example: Echo tool

Start with the simplest possible tool to understand the contract:

```lua
tool = {
    name = "echo",
    version = "0.1.0",
    description = "Echoes back the input message and context info",
    parameters = {
        {
            name = "message",
            type = "string",
            required = true,
            description = "The message to echo",
        },
    },
}

function tool.execute(params, context)
    log.info("Echo: " .. params.message)

    -- Use the context bridge to access the knowledge base
    local sources = context.sources()

    return {
        echo = "Echo: " .. params.message,
        source_count = #sources,
    }
end
```

```toml
[tools.script.echo]
path = "tools/echo.lua"
timeout = 5
```

### RAG-enriched Jira ticket creation

This tool searches the knowledge base for related context, then creates a Jira ticket with that context included:

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

```toml
[tools.script.create_jira_ticket]
path = "tools/create-jira-ticket.lua"
timeout = 30
jira_url = "https://mycompany.atlassian.net"
jira_project = "ENG"
jira_token = "${JIRA_API_TOKEN}"
```

### Post to Slack with context

```lua
tool = {
    name = "post_slack",
    version = "0.1.0",
    description = "Post a message to a Slack channel with relevant context from the knowledge base",
    parameters = {
        {
            name = "channel",
            type = "string",
            required = true,
            description = "Slack channel ID (e.g., C01ABCDEF)",
        },
        {
            name = "message",
            type = "string",
            required = true,
            description = "Message text to post",
        },
        {
            name = "include_context",
            type = "boolean",
            required = false,
            default = true,
            description = "Include related docs from the knowledge base",
        },
    },
}

function tool.execute(params, context)
    local text = params.message

    -- Optionally enrich with context
    if params.include_context then
        local results = context.search(params.message, { limit = 3, mode = "hybrid" })
        if #results > 0 then
            text = text .. "\n\n📚 *Related docs:*"
            for _, r in ipairs(results) do
                if r.source_url then
                    text = text .. "\n• <" .. r.source_url .. "|" .. r.title .. ">"
                else
                    text = text .. "\n• " .. r.title
                end
            end
        end
    end

    local resp = http.post(
        "https://slack.com/api/chat.postMessage",
        json.encode({
            channel = params.channel,
            text = text,
        }),
        {
            headers = {
                ["Authorization"] = "Bearer " .. context.config.slack_token,
                ["Content-Type"] = "application/json",
            },
        }
    )

    local data = json.decode(resp.body)
    return {
        ok = data.ok,
        ts = data.ts,
        channel = params.channel,
    }
end
```

### Create a GitHub issue

```lua
tool = {
    name = "create_github_issue",
    version = "0.1.0",
    description = "Create a GitHub issue with RAG-enriched context",
    parameters = {
        {
            name = "title",
            type = "string",
            required = true,
            description = "Issue title",
        },
        {
            name = "body",
            type = "string",
            required = false,
            description = "Issue body text",
        },
        {
            name = "labels",
            type = "string",
            required = false,
            description = "Comma-separated labels (e.g., 'bug,high-priority')",
        },
    },
}

function tool.execute(params, context)
    -- Search for related context
    local results = context.search(params.title, { limit = 3, mode = "hybrid" })

    local body = params.body or ""
    if #results > 0 then
        body = body .. "\n\n---\n\n### Related Context (auto-generated)\n\n"
        for _, r in ipairs(results) do
            body = body .. "- [" .. r.title .. "](" .. (r.source_url or "") .. ") (score: "
                .. string.format("%.2f", r.score) .. ")\n"
        end
    end

    -- Parse labels
    local labels = {}
    if params.labels then
        for label in params.labels:gmatch("[^,]+") do
            table.insert(labels, label:match("^%s*(.-)%s*$"))
        end
    end

    local resp = http.post(
        string.format("https://api.github.com/repos/%s/%s/issues",
            context.config.owner, context.config.repo),
        json.encode({
            title = params.title,
            body = body,
            labels = labels,
        }),
        {
            headers = {
                ["Authorization"] = "Bearer " .. context.config.github_token,
                ["Content-Type"] = "application/json",
                ["Accept"] = "application/vnd.github.v3+json",
            },
        }
    )

    local issue = json.decode(resp.body)
    return {
        number = issue.number,
        url = issue.html_url,
        related_docs = #results,
    }
end
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

The `context` argument in `tool.execute(params, context)` provides access to the Context Harness knowledge base:

**`context.search(query, opts?)`** — Search the knowledge base.

```lua
local results = context.search("auth flow", {
    mode = "hybrid",   -- "keyword" | "semantic" | "hybrid"
    limit = 5,
    source = "git",    -- Filter by source name
})
-- Returns: [{title, score, snippet, source_url, source, source_id}, ...]
```

**`context.get(id)`** — Retrieve a full document by UUID.

```lua
local doc = context.get("a1b2c3d4-...")
-- Returns: {id, title, body, source, source_id, source_url, updated_at}
```

**`context.sources()`** — List all configured data sources and their status.

```lua
local sources = context.sources()
-- Returns: [{source, document_count, chunk_count}, ...]
```

**`context.config`** — Tool-specific config from `ctx.toml` (env vars already expanded).

```lua
local api_key = context.config.api_key
local project = context.config.project_id
```

### HTTP endpoints

**`GET /tools/list`** — Discover all registered tools with their schemas:

```bash
$ curl -s localhost:7331/tools/list | jq '.tools[] | {name, description, builtin}'
{"name": "search", "description": "Search indexed documents", "builtin": true}
{"name": "get", "description": "Retrieve full document by ID", "builtin": true}
{"name": "sources", "description": "List configured data sources", "builtin": true}
{"name": "create_jira_ticket", "description": "Create a Jira ticket", "builtin": false}
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

| Status | Meaning |
|--------|---------|
| `200` | Success — `{"result": {...}}` |
| `400` | Parameter validation failed |
| `404` | Unknown tool name |
| `408` | Lua script timed out |
| `500` | Script execution error |

### CLI commands

```bash
# Scaffold a new tool:
$ ctx tool init post-slack
Created tools/post-slack.lua

# Test with sample params:
$ ctx tool test tools/echo.lua --param message="hello world"
Tool: echo v0.1.0
  Description: Echoes back the input message
  Parameters: message (string, required)
Result:
{
  "echo": "Echo: hello world",
  "source_count": 2
}

# Test with config from ctx.toml:
$ ctx tool test tools/create-jira-ticket.lua \
    --param title="Fix bug" \
    --param priority="High" \
    --source create_jira_ticket

# List all configured tools:
$ ctx tool list
Built-in tools:
  search           Search indexed documents
  get              Retrieve full document by ID
  sources          List configured data sources

Lua tools:
  echo             Echoes back the input message
  create_jira_ticket  Create a Jira ticket enriched with context
  post_slack       Post a message to Slack
```
