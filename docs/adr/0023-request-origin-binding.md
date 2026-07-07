# ADR-0023: Request Origin via HTTP Header (with Initialize-Param Fallback)

**Status:** Proposed
**Date:** 2026-06-09

## Context

The multi-workspace MCP router ([SPEC-0014](../spec/0014-multi-workspace-mcp-router.md),
[DESIGN-0008](../design/0008-multi-workspace-mcp-router.md)) routes built-in
tools to a workspace by an explicit selector, a qualified id, or a configured
default. [DESIGN-0009](../design/0009-workspace-scoped-extensions.md) extends
this so that the *set of tools, agents, and prompts* a session sees is the union
of a shared global layer and the originating workspace's layer. That requires
the server to know **which workspace a request comes from**.

The MCP transport in use is Streamable HTTP ([ADR-0009](0009-mcp-streamable-http-transport.md)).
A `tools/call` carries a tool name and arguments and nothing about the client's
working directory or project. The server therefore has **no ambient signal** for
the originating workspace — it must be told. The open question this ADR closes is
*how* a client tells it: the mechanism must be settable in real MCP client
configurations (Cursor, Claude Desktop, Claude Code), must not depend on the LLM
populating it per call, and must not let a client point the server at an
arbitrary on-disk config (the allowlist boundary from DESIGN-0008).

## Decision

**Request origin is carried by an HTTP header, `X-Context-Harness-Workspace`,
set once per MCP client configuration.** An Axum middleware layer extracts the
header and associates its value with the MCP session (and reads it per request
for the stateless REST endpoints). `McpBridge` reads the session's origin when
resolving the effective tool/prompt set and when applying built-in selector
precedence.

Specifics:

- **Value:** a registered workspace id, or an absolute workspace `root` path.
  The value is resolved against the workspace registry and **validated against
  the allowlist**. An unrecognized value yields globals-only resolution; the
  server never loads a config outside the registry to satisfy it
  (SPEC-0014 requirements 66–68).
- **Secondary, optional source:** an MCP `initialize` request `_meta` field,
  `contextHarness/workspace`, carrying the same value. It is honored only when
  the header is absent. If both are present, the **header wins**.
- **Session lifetime:** origin is resolved once at session initialization and is
  immutable for the life of the session. Switching workspace means a new session
  (reconnect) or an explicit `workspace` selector on the call.
- **REST parity:** the REST `/tools/*` endpoints read the same header per
  request; REST callers may instead pass `workspace` in the request body.
- **Scope:** applies in multi-workspace mode only. In compatibility mode there is
  one workspace and the header is ignored.

The header is primary because, in practice, MCP clients commonly expose a
per-server `headers` map but rarely expose a way to inject custom `initialize`
parameters. The header is therefore the mechanism that is actually settable
across the target clients today; the `initialize` path is kept as a
protocol-native convenience for clients that prefer it.

## Alternatives Considered

**Per-call origin argument (model-supplied).** Add a `cwd` / `project_root`
argument to every tool call. Rejected: it relies on the LLM to populate the
correct value on every call. It is unreliable, invisible to the user, and easily
omitted. Origin should be declarative client configuration, not model output.

**Path-based route (`/mcp/{workspace}`).** Encode the workspace in the URL and
run one logical endpoint per workspace path. Rejected as primary: it fragments
the single-URL property that ADR-0009 deliberately provides, multiplies client
config entries, and complicates the current single `nest_service("/mcp", …)`
mount. It also offers no capability the header lacks. It remains a possible
future addition for clients that cannot set headers.

**MCP `initialize` parameter as the primary mechanism.** Protocol-native and
survives within the session. Rejected as primary because most current MCP
clients do not let users set arbitrary `initialize` parameters, whereas they do
let users set request headers. Demoting it to an optional fallback keeps the
protocol-native door open without depending on client support that does not yet
exist broadly.

**Server-side inference from filesystem.** Have the server guess the project
from its own state. Rejected: there is no server-side notion of the client's
working directory, and any disk-scanning inference re-introduces the discovery
blast radius rejected in DESIGN-0008.

**One server/process per workspace.** Make origin implicit in the connection by
running a process or port per workspace. Rejected for the same reasons as in
DESIGN-0008: it re-fragments client configuration and defeats the
single-endpoint goal.

## Consequences

- **Single URL preserved.** Clients still point at one `http://host:port/mcp`;
  origin is one extra header line in the same config block. The single-URL
  benefit from ADR-0009 is retained.
- **Works with real clients.** The header is the knob current MCP clients
  actually expose, so the feature is usable on day one rather than gated on
  client support for custom `initialize` params.
- **Allowlist boundary intact.** Because the header value is validated against
  the registry, a client cannot cause the server to load an arbitrary
  `.ctx/config.toml`. An unknown value safely degrades to globals-only.
- **Reliability.** Origin is set in configuration, not produced by the model per
  call, so it does not drift or get omitted mid-conversation.
- **Accepted downside — mild single-URL erosion.** Each project's client config
  now differs by one header value. This is additive: project-scoped clients
  already differ per project, and a client that sets no header still works via
  explicit `workspace` selectors and the configured default.
- **Accepted downside — clients without header or init-param support** get no
  ambient origin; they must pass `workspace` explicitly or rely on
  `[defaults].workspace`. This is acceptable and degrades gracefully.
- **CORS.** The server already sets permissive CORS (`allow_headers(Any)`), which
  admits the custom request header and its preflight. No CORS change is required;
  this should be revisited if CORS is ever tightened.
- **Session state.** Binding origin to a session is a narrow, deliberate
  exception to the stateless posture of [ADR-0014](0014-stateless-agent-architecture.md):
  it is lightweight session *routing* context, not agent or conversation state,
  and it is immutable for the session.
- **Immutability for v1.** Because origin cannot change within a session, the
  server need not emit MCP `tools/list_changed` for origin changes initially.
  Allowing mid-session origin changes is a future revision that would require
  that notification.

## References

- [SPEC-0014: Multi-Workspace MCP Router](../spec/0014-multi-workspace-mcp-router.md) — Phase 3, requirements 65–82
- [DESIGN-0009: Workspace-Scoped Extensions and Request Origin](../design/0009-workspace-scoped-extensions.md)
- [DESIGN-0008: Multi-Workspace MCP Router](../design/0008-multi-workspace-mcp-router.md)
- [PRD-0011: Multi-Workspace MCP Router](../prd/0011-multi-workspace-mcp-router.md) — Phase 3
- [ADR-0009: MCP Streamable HTTP Transport](0009-mcp-streamable-http-transport.md)
- [ADR-0014: Stateless Agent Architecture](0014-stateless-agent-architecture.md)
- [ADR-0022: XDG Base Directory Compliance](0022-xdg-base-directory-compliance.md)
