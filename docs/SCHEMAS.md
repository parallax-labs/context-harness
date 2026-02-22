# Context Harness — JSON Schemas

This document defines the required JSON shape for all MCP tool responses.
All responses MUST match these schemas exactly.

Unless otherwise specified:
- All request/response bodies are UTF-8 JSON
- All timestamps are ISO8601 strings in UTC (RFC3339)
- UUIDs are lowercase strings

---

## Tool: context.search

### Endpoint

`POST /tools/search`

### Request Schema

```json
{
  "query": "string",
  "mode": "keyword | semantic | hybrid",
  "limit": 12,
  "filters": {
    "source": "string | null",
    "tags": ["string"],
    "since": "ISO8601 | null",
    "until": "ISO8601 | null"
  }
}
```

### Response Schema

```json
{
  "results": [
    {
      "id": "uuid",
      "score": 0.92,
      "title": "string | null",
      "source": "string",
      "source_id": "string",
      "updated_at": "ISO8601",
      "snippet": "string",
      "source_url": "string | null"
    }
  ]
}
```

---

## Tool: context.get

### Endpoint

`POST /tools/get`

### Request Schema

```json
{
  "id": "uuid"
}
```

### Response Schema

```json
{
  "id": "uuid",
  "source": "string",
  "source_id": "string",
  "source_url": "string | null",
  "title": "string | null",
  "author": "string | null",
  "created_at": "ISO8601",
  "updated_at": "ISO8601",
  "content_type": "string",
  "body": "string",
  "metadata": {},
  "chunks": [
    {
      "index": 0,
      "text": "string"
    }
  ]
}
```

---

## Tool: context.sources

### Endpoint

`GET /tools/sources`

### Response Schema

```json
{
  "sources": [
    {
      "name": "string",
      "configured": true,
      "healthy": true,
      "notes": "string | null"
    }
  ]
}
```

---

---

## Tool: tools.list

### Endpoint

`GET /tools/list`

### Response Schema

```json
{
  "tools": [
    {
      "name": "string",
      "description": "string",
      "builtin": true,
      "parameters": {
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "string"
          }
        },
        "required": ["query"]
      }
    }
  ]
}
```

---

## Tool: tools.{name} (Dynamic Lua Tools)

### Endpoint

`POST /tools/{name}`

### Request Schema

Tool-specific parameters as a flat JSON object:

```json
{
  "param1": "value1",
  "param2": 42
}
```

### Response Schema (Success)

```json
{
  "result": { ... }
}
```

The `result` value is tool-specific — it is whatever the Lua
`tool.execute()` function returns.

### Response Schema (Script Error)

```json
{
  "error": {
    "code": "tool_error",
    "message": "string"
  }
}
```

---

## Agents: agents.list

### Endpoint

`GET /agents/list`

### Response Schema

```json
{
  "agents": [
    {
      "name": "string",
      "description": "string",
      "tools": ["string"],
      "source": "toml | lua | rust",
      "arguments": [
        {
          "name": "string",
          "description": "string",
          "required": false
        }
      ]
    }
  ]
}
```

---

## Agents: agents.{name}.prompt

### Endpoint

`POST /agents/{name}/prompt`

### Request Schema

Agent arguments as a flat JSON object:

```json
{
  "service": "payments-api",
  "severity": "P1"
}
```

### Response Schema (Success)

```json
{
  "system": "string",
  "tools": ["string"],
  "messages": [
    {
      "role": "user | assistant | system",
      "content": "string"
    }
  ]
}
```

### Response Schema (Error)

```json
{
  "error": {
    "code": "not_found | tool_error",
    "message": "string"
  }
}
```

---

## Error Schema (All Endpoints)

```json
{
  "error": {
    "code": "string",
    "message": "string"
  }
}
```

### Error Codes

- `bad_request` — malformed JSON, missing required fields
- `not_found` — document id not present, or unknown tool name
- `not_configured` — connector missing config
- `embeddings_disabled` — semantic/hybrid requires embeddings
- `tool_error` — Lua tool script raised an error
- `timeout` — tool execution exceeded configured timeout
- `internal` — unexpected error

### HTTP Status Codes

- 200 for success
- 400 for bad_request, embeddings_disabled
- 404 for not_found
- 408 for timeout
- 500 for internal, tool_error

