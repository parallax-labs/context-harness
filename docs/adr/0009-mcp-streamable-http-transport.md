# ADR-0009: MCP Streamable HTTP Transport

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness needs to integrate with AI development tools that support
the Model Context Protocol (MCP), including Cursor, Claude Desktop, and
other MCP-compatible clients. MCP defines how AI assistants discover and
invoke tools and prompts from external servers.

MCP supports multiple transport mechanisms:

- **stdio** — server runs as a subprocess, communicating via stdin/stdout
- **SSE (Server-Sent Events)** — HTTP-based, server pushes events to client
- **Streamable HTTP** — the latest MCP transport, supporting bidirectional
  JSON-RPC over HTTP with optional streaming

The transport choice affects how clients connect, whether state can be shared
across clients, and operational complexity.

## Decision

Use **MCP Streamable HTTP** transport at the `/mcp` endpoint, implemented
via the `rmcp` crate with the `transport-streamable-http-server` feature.

The MCP endpoint is served by the **same Axum HTTP server** that hosts the
REST API. A single `ctx serve mcp` command starts one server that handles:

- REST endpoints (`POST /tools/search`, `GET /tools/list`, etc.)
- MCP JSON-RPC (`POST /mcp`, with optional SSE streaming)
- Health check (`GET /health`)

The `McpBridge` struct in `src/mcp.rs` implements `rmcp::ServerHandler` and
maps:

- `tools/list` → `ToolRegistry::list()` with OpenAI-compatible JSON schemas
- `tools/call` → `ToolRegistry::execute()` with error mapping to
  `CallToolResult`
- `prompts/list` → `AgentRegistry::list()`
- `prompts/get` → `Agent::resolve()` with argument forwarding

Tool errors are returned as `CallToolResult::error` (text content), not
JSON-RPC errors, following MCP conventions.

CORS is set to `Any` to support browser-based and cross-origin MCP clients.

## Alternatives Considered

**stdio subprocess transport.** The client spawns `ctx serve mcp` as a child
process and communicates via stdin/stdout. This is simpler for single-client
setups but has significant drawbacks:
- Each client spawns a separate process with its own SQLite connection,
  losing shared state.
- No way to share a running server across multiple clients (e.g., Cursor
  and a custom integration simultaneously).
- Subprocess management varies across platforms and editors.
- Cannot serve REST endpoints for non-MCP clients.

**SSE-only transport.** HTTP-based with server-push via Server-Sent Events.
Widely supported but unidirectional — the server cannot initiate requests to
the client. Streamable HTTP subsumes SSE capabilities while adding
bidirectional support. The `rmcp` crate implements Streamable HTTP with
SSE fallback.

**Custom WebSocket protocol.** Full-duplex communication with low overhead.
However, MCP does not define a WebSocket transport, so this would be
non-standard and incompatible with existing MCP clients.

**Separate MCP and REST servers.** Run two server processes on different
ports. Doubles operational complexity and resource usage with no benefit.
Axum's router naturally supports both on the same port.

## Consequences

- A single `ctx serve mcp` command provides both MCP and REST access on
  one port (default `127.0.0.1:7331`).
- Multiple MCP clients can connect to the same server simultaneously,
  sharing the SQLite connection pool and tool/agent registries.
- Client configuration is a single URL (`http://127.0.0.1:7331/mcp`)
  rather than a command + arguments for stdio.
- The server must be running before clients connect (unlike stdio, which
  starts the server on demand). This is mitigated by running `ctx serve mcp`
  as a background service or via the user's shell profile.
- Streamable HTTP is the newest MCP transport. Some older MCP clients may
  not support it, but `rmcp` includes SSE fallback for compatibility.
- The `McpBridge` adapter pattern keeps MCP protocol details isolated from
  the core tool and agent logic, which remains transport-agnostic.
