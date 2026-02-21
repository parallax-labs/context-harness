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
- `not_found` — document id not present
- `not_configured` — connector missing config
- `embeddings_disabled` — semantic/hybrid requires embeddings
- `internal` — unexpected error

### HTTP Status Codes

- 200 for success
- 400 for bad_request
- 404 for not_found
- 500 for internal

