+++
title = "MCP Server API"
description = "HTTP API reference for AI agent integration."
weight = 2
+++

The MCP server (`ctx serve mcp`) exposes both a native MCP Streamable HTTP endpoint and a REST API. MCP clients (Cursor, Claude Desktop, etc.) connect to the `/mcp` endpoint using the MCP JSON-RPC protocol. Custom integrations can use the REST endpoints directly. CORS is enabled by default for browser-based clients.

### Starting the server

```bash
$ ctx serve mcp --config ./config/ctx.toml
Loaded 2 Lua tool(s):
  POST /tools/echo — Echoes back the input message
  POST /tools/create_jira_ticket — Create a Jira ticket enriched with related context
MCP server listening on http://127.0.0.1:7331
  MCP endpoint: http://127.0.0.1:7331/mcp
```

The bind address is configurable:

```toml
[server]
bind = "127.0.0.1:7331"    # Local only (default)
# bind = "0.0.0.0:7331"    # Docker / remote access
```

### MCP Streamable HTTP endpoint

The `/mcp` endpoint speaks the [MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports#streamable-http) transport — JSON-RPC over HTTP with server-sent events for streaming. This is what MCP clients like Cursor and Claude Desktop connect to.

**Connect from Cursor** (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

**What's exposed via MCP:**

| MCP Method | Description |
|-----------|-------------|
| `tools/list` | All registered tools (built-in + Lua + Rust) |
| `tools/call` | Execute any tool by name |
| `prompts/list` | All registered agents as MCP prompts |
| `prompts/get` | Resolve an agent's system prompt |

**Test with curl:**

```bash
$ curl -X POST http://127.0.0.1:7331/mcp \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
```

### REST endpoint reference

#### `POST /tools/search`

Full-text, semantic, or hybrid search across the knowledge base.

```bash
$ curl -s localhost:7331/tools/search \
    -H "Content-Type: application/json" \
    -d '{
      "query": "authentication",
      "mode": "hybrid",
      "limit": 5
    }' | jq .
```

**Response:**

```json
{
  "results": [
    {
      "id": "a1b2c3d4-e5f6-...",
      "source": "git",
      "source_id": "docs/auth.md",
      "title": "Authentication Guide",
      "score": 0.94,
      "snippet": "JWT tokens are signed with RS256 and rotate every...",
      "source_url": "https://github.com/acme/platform/blob/main/docs/auth.md"
    }
  ]
}
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `query` | string | **required** | Search query text |
| `mode` | string | `"keyword"` | `"keyword"`, `"semantic"`, or `"hybrid"` |
| `limit` | integer | from config | Max results to return |
| `source` | string | all | Filter by source name (e.g., `"git"`, `"script:jira"`) |

#### `POST /tools/get`

Retrieve a full document by UUID.

```bash
$ curl -s localhost:7331/tools/get \
    -H "Content-Type: application/json" \
    -d '{"id": "a1b2c3d4-e5f6-..."}' | jq .
```

**Response:**

```json
{
  "id": "a1b2c3d4-e5f6-...",
  "source": "git",
  "source_id": "docs/auth.md",
  "source_url": "https://github.com/acme/platform/blob/main/docs/auth.md",
  "title": "Authentication Guide",
  "body": "# Authentication Guide\n\nJWT tokens are signed with...",
  "updated_at": "2024-01-15T10:30:00Z"
}
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | string | **required** — Document UUID from search results |

#### `GET /tools/sources`

List all configured data sources and their document/chunk counts.

```bash
$ curl -s localhost:7331/tools/sources | jq .
```

**Response:**

```json
{
  "sources": [
    {
      "source": "filesystem",
      "document_count": 45,
      "chunk_count": 213
    },
    {
      "source": "git",
      "document_count": 89,
      "chunk_count": 412
    },
    {
      "source": "script:jira",
      "document_count": 234,
      "chunk_count": 234
    }
  ]
}
```

#### `GET /tools/list`

Discover all registered tools (built-in, Lua, and custom Rust) with OpenAI-compatible JSON Schema. This is what AI agents use to know what tools are available:

```bash
$ curl -s localhost:7331/tools/list | jq '.tools[] | {name, description, builtin}'
```

**Response:**

```json
{
  "tools": [
    {
      "name": "search",
      "description": "Search indexed documents by keyword, semantic, or hybrid query",
      "builtin": true,
      "parameters": {
        "type": "object",
        "properties": {
          "query": { "type": "string", "description": "Search query" },
          "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"] },
          "limit": { "type": "integer" },
          "source": { "type": "string" }
        },
        "required": ["query"]
      }
    },
    {
      "name": "get",
      "description": "Get full document content by ID",
      "builtin": true,
      "parameters": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "description": "Document UUID" }
        },
        "required": ["id"]
      }
    },
    {
      "name": "sources",
      "description": "List all configured data sources",
      "builtin": true,
      "parameters": { "type": "object", "properties": {} }
    },
    {
      "name": "create_jira_ticket",
      "description": "Create a Jira ticket enriched with related context",
      "builtin": false,
      "parameters": {
        "type": "object",
        "properties": {
          "title": { "type": "string", "description": "Ticket title" },
          "priority": { "type": "string", "enum": ["Low", "Medium", "High", "Critical"] }
        },
        "required": ["title"]
      }
    }
  ]
}
```

#### `POST /tools/{name}`

Call any registered tool by name. Works for both built-in and Lua-defined tools.

```bash
# Call a built-in tool
$ curl -s -X POST localhost:7331/tools/search \
    -H "Content-Type: application/json" \
    -d '{"query": "error handling"}'

# Call a Lua tool
$ curl -s -X POST localhost:7331/tools/create_jira_ticket \
    -H "Content-Type: application/json" \
    -d '{"title": "Fix auth bug", "priority": "High"}'
```

**Response:**

```json
{"result": {"key": "ENG-1234", "url": "https://acme.atlassian.net/browse/ENG-1234", "related_docs": 3}}
```

| Status | Meaning |
|--------|---------|
| `200` | Success — `{"result": {...}}` |
| `400` | Parameter validation failed |
| `404` | Unknown tool name |
| `408` | Lua script timed out |
| `500` | Script execution error |

#### `GET /agents/list`

Discover all registered agents with their metadata, tool lists, and argument schemas:

```bash
$ curl -s localhost:7331/agents/list | jq '.agents[] | {name, description, tools, source}'
```

**Response:**

```json
{
  "agents": [
    {
      "name": "code-reviewer",
      "description": "Reviews code changes against project conventions",
      "tools": ["search", "get"],
      "source": "toml",
      "arguments": []
    },
    {
      "name": "incident-responder",
      "description": "Helps triage production incidents with relevant runbooks",
      "tools": ["search", "get", "create_jira_ticket"],
      "source": "lua",
      "arguments": [
        { "name": "service", "description": "The service experiencing the incident", "required": false },
        { "name": "severity", "description": "Incident severity (P1, P2, P3)", "required": false }
      ]
    }
  ]
}
```

#### `POST /agents/{name}/prompt`

Resolve an agent's system prompt. For Lua agents, this executes the script's `agent.resolve()` function with access to the context bridge.

```bash
$ curl -s localhost:7331/agents/incident-responder/prompt \
    -H "Content-Type: application/json" \
    -d '{"service": "payments-api", "severity": "P1"}' | jq .
```

**Response:**

```json
{
  "system": "You are an incident responder for the payments-api service (P1 severity)...",
  "tools": ["search", "get", "create_jira_ticket"],
  "messages": [
    {
      "role": "assistant",
      "content": "I'm ready to help with the P1 payments-api incident..."
    }
  ]
}
```

| Parameter | Type | Description |
|-----------|------|-------------|
| (body) | object | Agent-specific arguments as key-value pairs |

| Status | Meaning |
|--------|---------|
| `200` | Success |
| `404` | Agent not found |
| `500` | Lua resolve() failed |
| `408` | Lua resolve() timed out |

#### `GET /health`

Health check endpoint. Returns `200 OK` with `{"status": "ok"}`.

```bash
$ curl -s localhost:7331/health
{"status":"ok"}
```

### Connecting to AI agents

All MCP clients connect to `http://127.0.0.1:7331/mcp` (the Streamable HTTP endpoint). The REST endpoints above are available for custom integrations that don't speak MCP.

See the [Agent Integration](@/docs/guides/agent-integration.md) guide for step-by-step setup with:

- **Cursor** — workspace-level or global MCP config → `http://127.0.0.1:7331/mcp`
- **Claude Desktop** — MCP server URL → `http://127.0.0.1:7331/mcp`
- **Continue.dev** — MCP server or REST context provider
- **OpenClaw / Open Interpreter** — HTTP tool calling (REST API)
- **Custom agents** — any language, any framework
