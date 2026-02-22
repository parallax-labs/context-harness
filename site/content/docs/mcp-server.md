+++
title = "MCP Server"
description = "HTTP API reference for the Context Harness MCP server."
weight = 8

[extra]
sidebar_label = "MCP Server"
sidebar_group = "Reference"
sidebar_order = 8
+++

The MCP server (`ctx serve mcp`) exposes an HTTP API that AI agents (Cursor, Claude, browser LLMs) can use to search documents, retrieve full content, discover tools, and execute custom actions.

## Starting the Server

```bash
$ ctx serve mcp --config ./config/ctx.toml
# Listening on 127.0.0.1:7331
```

The server binds to the address configured in `[server].bind`. CORS is open by default for browser-based clients.

## Endpoints

### `POST /tools/search`

Search the knowledge base.

**Request:**

```json
{
  "query": "authentication",
  "mode": "hybrid",
  "limit": 10,
  "source": "filesystem"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `query` | string | required | Search query |
| `mode` | string | `"keyword"` | `"keyword"`, `"semantic"`, or `"hybrid"` |
| `limit` | integer | from config | Max results |
| `source` | string | all | Filter by source name |

**Response:**

```json
{
  "results": [
    {
      "id": "uuid",
      "source": "filesystem",
      "source_id": "docs/auth.md",
      "title": "Authentication Guide",
      "score": 0.94,
      "snippet": "JWT tokens are signed with...",
      "source_url": null
    }
  ]
}
```

### `POST /tools/get`

Retrieve a full document by ID.

**Request:**

```json
{ "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890" }
```

**Response:**

```json
{
  "id": "a1b2c3d4-...",
  "source": "git",
  "source_id": "docs/auth.md",
  "title": "Authentication Guide",
  "body": "Full document content...",
  "source_url": "https://github.com/...",
  "updated_at": "2025-01-15T10:30:00Z"
}
```

### `GET /tools/sources`

List configured data sources.

**Response:**

```json
{
  "sources": [
    { "name": "filesystem", "configured": true, "healthy": true, "doc_count": 47 },
    { "name": "git", "configured": true, "healthy": true, "doc_count": 23 }
  ]
}
```

### `GET /tools/list`

Discover all registered tools (built-in + Lua extensions).

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
          "limit": { "type": "integer" }
        },
        "required": ["query"]
      }
    }
  ]
}
```

### `POST /tools/{name}`

Call a registered tool by name.

**Request:** JSON object matching the tool's parameter schema.

**Response:**

```json
{ "result": { ... } }
```

**Error codes:**
- `400` — parameter validation failed
- `404` — unknown tool name
- `408` — Lua script timed out
- `500` — script execution error

### `GET /health`

Health check endpoint.

**Response:** `200 OK`

```json
{ "status": "ok" }
```

## Cursor Integration

Add the MCP server to Cursor's settings:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://localhost:7331"
    }
  }
}
```

Then in Cursor, you can ask:

> "Search the docs for deployment procedures"

The agent will call `POST /tools/search` automatically.

