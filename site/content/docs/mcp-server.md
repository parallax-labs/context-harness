+++
title = "MCP Server"
description = "HTTP API endpoints for AI agent integration."
weight = 8

[extra]
sidebar_label = "MCP Server"
sidebar_group = "Reference"
sidebar_order = 8
+++

The MCP server (`ctx serve mcp`) exposes an HTTP API that AI agents can use to search documents, retrieve content, discover tools, and execute custom actions. CORS is open by default for browser-based clients.

### Starting the server

```bash
$ ctx serve mcp
Loaded 2 Lua tool(s):
  POST /tools/echo — Echoes back the input message
  POST /tools/create_jira_ticket — Create a Jira ticket enriched with related context
Listening on 127.0.0.1:7331
```

### `POST /tools/search`

Search the knowledge base.

```bash
$ curl -s localhost:7331/tools/search \
    -H "Content-Type: application/json" \
    -d '{"query": "authentication", "mode": "hybrid", "limit": 5}' | jq .

{
  "results": [
    {
      "id": "a1b2c3d4-...",
      "source": "git",
      "source_id": "docs/auth.md",
      "title": "Authentication Guide",
      "score": 0.94,
      "snippet": "JWT tokens are signed with RS256...",
      "source_url": "https://github.com/acme/platform/blob/main/docs/auth.md"
    }
  ]
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `query` | string | required | Search query |
| `mode` | string | `"keyword"` | `"keyword"`, `"semantic"`, or `"hybrid"` |
| `limit` | integer | from config | Max results |
| `source` | string | all | Filter by source name |

### `POST /tools/get`

Retrieve a full document by ID.

```bash
$ curl -s localhost:7331/tools/get \
    -d '{"id": "a1b2c3d4-..."}' | jq '{title, source, source_url}'
```

### `GET /tools/sources`

List configured data sources and their status.

### `GET /tools/list`

Discover all registered tools (built-in + Lua extensions) with OpenAI-compatible JSON Schema:

```bash
$ curl -s localhost:7331/tools/list | jq '.tools[0]'
{
  "name": "search",
  "description": "Search indexed documents by keyword, semantic, or hybrid query",
  "builtin": true,
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "Search query" },
      "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"] },
      "limit": { "type": "integer" }
    },
    "required": ["query"]
  }
}
```

### `POST /tools/{name}`

Call any registered tool by name with JSON parameters.

```bash
$ curl -X POST localhost:7331/tools/create_jira_ticket \
    -H "Content-Type: application/json" \
    -d '{"title": "Fix auth bug", "priority": "High"}'

{"result": {"key": "ENG-1234", "url": "...", "related_docs": 3}}
```

| Status | Meaning |
|--------|---------|
| `200` | Success — `{"result": {...}}` |
| `400` | Parameter validation failed |
| `404` | Unknown tool name |
| `408` | Lua script timed out |
| `500` | Script execution error |

### `GET /health`

Health check. Returns `200 OK` with `{"status": "ok"}`.

### Cursor integration

Add Context Harness to Cursor's MCP settings:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://localhost:7331"
    }
  }
}
```

Then you can ask Cursor: *"Search the docs for deployment procedures"* and the agent will call `POST /tools/search` automatically.
