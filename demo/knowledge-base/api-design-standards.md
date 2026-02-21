# API Design Standards

All public and internal APIs at Acme must follow these standards.

---

## RESTful API Conventions

### URL Structure

```
/{version}/{resource}/{id}/{sub-resource}
```

Examples:
- `GET /v1/orders` — list orders
- `GET /v1/orders/123` — get order by ID
- `POST /v1/orders` — create order
- `PUT /v1/orders/123` — update order
- `DELETE /v1/orders/123` — delete order
- `GET /v1/orders/123/items` — list items for order

### HTTP Methods

| Method | Usage | Idempotent |
|--------|-------|------------|
| GET | Read resource(s) | Yes |
| POST | Create resource | No |
| PUT | Full update | Yes |
| PATCH | Partial update | Yes |
| DELETE | Remove resource | Yes |

### Response Codes

| Code | Usage |
|------|-------|
| 200 | Success |
| 201 | Created (POST) |
| 204 | No Content (DELETE) |
| 400 | Bad Request — validation failed |
| 401 | Unauthorized — missing/invalid token |
| 403 | Forbidden — insufficient permissions |
| 404 | Not Found |
| 409 | Conflict — duplicate resource |
| 422 | Unprocessable Entity — business rule violation |
| 429 | Too Many Requests — rate limited |
| 500 | Internal Server Error |
| 503 | Service Unavailable |

---

## Pagination

All list endpoints must support cursor-based pagination:

```json
{
  "data": [...],
  "pagination": {
    "cursor": "eyJpZCI6MTIzfQ==",
    "has_more": true,
    "total_count": 1547
  }
}
```

Query parameters:
- `cursor` — opaque cursor from previous response
- `limit` — items per page (default: 20, max: 100)
- `sort` — field to sort by (default: `created_at`)
- `order` — `asc` or `desc` (default: `desc`)

---

## Error Format

All errors must follow this structure:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "Human-readable error message",
    "details": [
      {
        "field": "email",
        "message": "must be a valid email address",
        "code": "invalid_format"
      }
    ],
    "request_id": "req_abc123",
    "timestamp": "2025-10-15T14:30:00Z"
  }
}
```

### Standard Error Codes

- `validation_failed` — request body/params invalid
- `not_found` — resource does not exist
- `unauthorized` — authentication required
- `forbidden` — insufficient permissions
- `conflict` — resource already exists
- `rate_limited` — too many requests
- `internal_error` — unexpected server error

---

## Authentication

All APIs use JWT Bearer tokens issued by our OAuth2 server:

```
Authorization: Bearer eyJhbGciOiJSUzI1NiIs...
```

### Token Scopes

| Scope | Access |
|-------|--------|
| `read:orders` | Read order data |
| `write:orders` | Create/update orders |
| `admin:orders` | Delete orders, manage rules |
| `read:users` | Read user profiles |
| `admin:users` | Manage user accounts |

### Service-to-Service Authentication

Internal services use mTLS with short-lived certificates rotated every 24 hours. The service mesh (Istio) handles certificate management transparently.

---

## Rate Limiting

| Tier | Limit | Window |
|------|-------|--------|
| Standard | 100 req/min | Per API key |
| Premium | 1000 req/min | Per API key |
| Internal | 10000 req/min | Per service identity |

Rate limit headers in every response:
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 87
X-RateLimit-Reset: 1697376000
```

---

## Versioning

- URL-based versioning: `/v1/`, `/v2/`
- Major version increments only for breaking changes
- Deprecation: minimum 6-month sunset period with `Sunset` header
- Breaking changes require ADR and cross-team review

---

## OpenAPI Specification

Every API must have an OpenAPI 3.1 spec committed to the repo:

```
service-name/
├── api/
│   └── openapi.yaml
├── src/
└── tests/
    └── api-contract/
        └── contract_test.rs
```

Contract tests validate that the implementation matches the spec. CI blocks merge if specs diverge.

