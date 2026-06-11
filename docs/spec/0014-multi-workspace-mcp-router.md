# SPEC-0014: Multi-Workspace MCP Router

**Status:** Phase 1 implemented (built-in routing); Phases 2–3 not yet implemented
**Date:** 2026-06-09
**Scope:** MCP and REST routing across multiple Context Harness workspace
configurations and stores.

> **Implementation status (2026-06-11).** Phase 1 (built-in workspace routing)
> is implemented: the `--workspaces` opt-in, the registry + `ctx workspace
> add/list/remove`, router-aware `search`/`get`/`sources`, the `workspaces`
> discovery tool, qualified-id `get`, connector-secret redaction, and the
> loopback trust model below. **Deferred:** `workspace = "all"` fan-out
> (Phase 2 — currently returns `unsupported_workspace_selector`) and the
> Phase-3 request-origin / workspace-scoped extensions (requirements 65–82).

## Overview

This spec defines the behavior of a single Context Harness MCP server that can
route requests to multiple registered workspaces. Each workspace retains its own
effective configuration, SQLite store, connector status, embedding state, and
vector-index sidecar. The router is a runtime dispatch layer and SHALL NOT merge
workspace stores into a single canonical database.

Multi-workspace behavior is **additive and opt-in**. The server runs in
compatibility (single-workspace) mode by default and behaves exactly as the
pre-router server. Multi-workspace mode is activated only by an explicit serve
option, never implicitly by the presence of a registry file.

## Definitions

**Workspace** is a named Context Harness project or corpus with its own resolved
configuration and canonical SQLite store.

**Workspace id** is a stable, user-facing identifier for a workspace. Workspace
ids are ASCII strings matching `[A-Za-z0-9][A-Za-z0-9_-]*`.

**Workspace registry** is the user-level TOML file that lists known workspaces.

**Workspace runtime** is the in-memory runtime for one workspace, including its
resolved config, built-in tool context, optional registries, and health state.

**Workspace selector** is a request field that identifies one workspace, all
workspaces, or the default workspace.

**Qualified document id** is a document identifier that includes the workspace id
and document id in the form `<workspace-id>:<document-id>`.

**Compatibility mode** is the default single-workspace server mode. It resolves
one effective config and exposes the pre-router endpoints, tool names, and
response shapes unchanged.

**Multi-workspace mode** is the router mode activated by an explicit serve
option. It exposes workspace selection, workspace-labeled responses, and the
`workspaces` discovery tool.

## Requirements

### Workspace Registry

1. The global workspace registry SHALL be located at
   `$XDG_CONFIG_HOME/ctx/workspaces.toml`, respecting `CTX_CONFIG_DIR` and
   `XDG_CONFIG_HOME` as defined by [SPEC-0013](0013-config-resolution.md).
2. The workspace registry SHALL use this TOML shape:

   ```toml
   [defaults]
   workspace = "context_harness"
   bind = "127.0.0.1:7331"

   [workspaces.context_harness]
   root = "/absolute/path/to/context-harness"
   config = "/absolute/path/to/context-harness/.ctx/config.toml"
   enabled = true
   ```

   The `[defaults].bind` field is optional and controls the shared server bind
   address in multi-workspace mode (see requirement 16).

3. The `root` field SHALL be required and SHALL be an absolute path.
4. The `config` field MAY be omitted. When omitted, the runtime SHALL resolve
   the workspace configuration from `root` using the workspace-local resolution
   behavior in [SPEC-0013](0013-config-resolution.md). In this case the global
   config defaults SHALL merge into the resolved config as defined by
   [SPEC-0013](0013-config-resolution.md).
5. When `config` is present, it SHALL be an absolute path and SHALL be loaded as
   the sole config source with no global merge, preserving explicit config
   behavior. The differing merge behavior between requirements 4 and 5 SHALL be
   treated as intentional and documented for operators.
6. The `enabled` field MAY be omitted and SHALL default to `true`.
7. Disabled workspaces SHALL be listed by workspace discovery but SHALL reject
   search, get, sources, tool, and prompt calls.
8. Workspace ids SHALL be unique within the registry.
9. Invalid workspace ids, relative root paths, and relative config paths SHALL
   make the affected workspace invalid and unavailable.
10. The registry MAY be absent. The presence or absence of the registry SHALL
    NOT by itself determine the server mode. Mode is selected as defined in
    "Server Modes" below.

### Server Modes

11. `ctx serve mcp` SHALL run in compatibility (single-workspace) mode by
    default.
12. Multi-workspace mode SHALL be activated only by an explicit opt-in: the
    `--workspaces` flag on `ctx serve mcp`, optionally taking a registry path as
    `--workspaces[=<path>]`. Without `--workspaces`, the server SHALL run in
    compatibility mode even when a registry file exists.
13. The `--workspaces` flag SHALL NOT be combined with `--config` or
    `CTX_CONFIG`. Combining them SHALL be rejected with a startup error. When
    `--config` or `CTX_CONFIG` is set without `--workspaces`, the server SHALL
    run in compatibility mode and SHALL ignore any registry file.
14. The server mode SHALL be determined solely by the activation path (presence
    of `--workspaces`), not by the number of registered workspaces. A registry
    containing exactly one workspace SHALL still run in multi-workspace mode when
    activated with `--workspaces`.
15. In compatibility mode, the MCP endpoint, REST endpoints, tool names, and
    response shapes SHALL be byte-for-byte identical to the pre-router server.
    This is the additive invariant: an existing single-config deployment SHALL
    require no client configuration change.
16. In multi-workspace mode, the MCP endpoint SHALL remain `/mcp` and REST
    endpoints SHALL remain available on the same Axum server. The shared server
    SHALL bind to `[defaults].bind` from the registry, defaulting to
    `127.0.0.1:7331` when absent. Per-workspace `[server].bind` values SHALL be
    ignored for the shared endpoint in multi-workspace mode.
17. Response shape SHALL be determined by mode. Compatibility mode SHALL use the
    existing flat response shapes. Multi-workspace mode SHALL use the
    workspace-labeled shapes defined below.

### Workspace Selection

18. Built-in `search`, `get`, and `sources` tools SHALL accept an optional
    `workspace` field.
19. If `workspace` is omitted, the router SHALL use the default workspace.
20. If no default workspace is configured: when exactly one enabled workspace
    exists, the router SHALL use it; when more than one enabled workspace
    exists, the router SHALL return a `workspace_required` error.
21. The reserved workspace selector `all` SHALL mean every enabled workspace.
22. `workspace = "all"` SHALL be valid for `search` and `sources`.
23. `workspace = "all"` SHALL NOT be valid for `get` unless the `id` is a
    qualified document id.
24. An unknown workspace id SHALL return an `unknown_workspace` error.
25. A disabled workspace id SHALL return a `workspace_disabled` error.
26. An invalid or unhealthy workspace runtime SHALL return a
    `workspace_unavailable` error.

### Search

27. Single-workspace search SHALL execute against the selected workspace's
    existing search implementation.
28. Single-workspace search SHALL preserve the selected workspace's existing
    search ranking, result fields, retrieval configuration, embedding provider,
    and vector-index fallback behavior.
29. Search responses in multi-workspace mode SHALL include `workspace` on every
    result item.
30. Search responses in multi-workspace mode SHALL include `qualified_id` on
    every result item.
31. In compatibility mode, returned document ids MAY remain unqualified for
    backward compatibility.
32. For `workspace = "all"`, the router SHALL search each enabled workspace
    independently.
33. For `workspace = "all"`, the router MAY search workspaces concurrently and
    SHALL apply a per-workspace deadline so that one slow, locked, or unhealthy
    store does not block the overall response.
34. For `workspace = "all"`, the `limit` parameter SHALL apply per workspace.
    The router SHALL NOT impose a single global limit across workspaces.
35. For `workspace = "all"`, the router SHALL NOT compare raw BM25, cosine, or
    hybrid scores as if they were globally comparable across stores.
36. For `workspace = "all"`, the response SHALL preserve workspace identity for
    every item and SHALL group results by workspace.
37. Workspace search failure SHALL NOT silently remove that workspace from an
    `all` search response. The response SHALL include an error entry for each
    failed workspace, including workspaces that exceed the per-workspace
    deadline.

### Get

38. `get` SHALL retrieve documents from one selected workspace.
39. `get` SHALL accept a qualified document id and route it to the encoded
    workspace.
40. An id SHALL be treated as qualified only when it contains `:` and the
    substring before the first `:` matches a registered workspace id
    (`[A-Za-z0-9][A-Za-z0-9_-]*`). Otherwise the entire value SHALL be treated
    as a raw document id and routed to the selected or default workspace.
41. If both an explicit `workspace` field and a qualified document id are
    provided, they SHALL refer to the same workspace.
42. If an explicit `workspace` field conflicts with a qualified document id, the
    router SHALL return `workspace_id_conflict`.
43. `get` SHALL return `not_found` when the selected workspace is available but
    the document id does not exist in that workspace.

### Sources and Workspace Discovery

44. `sources` without `workspace` SHALL use the default workspace in
    compatibility-compatible mode.
45. `sources` with `workspace = "all"` SHALL return source status grouped by
    workspace.
46. The server SHALL expose a workspace discovery tool named `workspaces`.
47. The `workspaces` tool SHALL return workspace id, root path, enabled status,
    default status, and runtime health.
48. The `workspaces` tool SHALL NOT expose secret values from workspace configs.
49. `sources` and `workspaces` output SHALL redact resolved connector
    configuration that may contain secrets — including credentials, tokens, and
    env-expanded values — not only fields whose names literally contain
    `secret` or `token`.

### Tools and Prompts

50. Built-in tools SHALL be available in compatibility and multi-workspace
    modes.
51. In compatibility mode, all workspace-local Lua tools, Rust tools, and
    prompts SHALL be exposed exactly as in the pre-router server. The
    multi-workspace built-ins-only guarantee in requirement 54 SHALL NOT reduce
    what compatibility mode exposes.
52. Workspace-local Lua tools, Rust tools, and prompts SHALL remain scoped to
    the workspace runtime that loaded them.
53. If multiple workspaces expose local tools or prompts with the same name, the
    router SHALL avoid ambiguous global registration.
54. Until the namespacing contract is implemented, multi-workspace mode SHALL
    only guarantee routing for built-in tools and workspace discovery. That
    contract is defined in "Phase 3: Workspace-Scoped Extensions and Request
    Origin" (requirements 65–82) and [DESIGN-0009](../design/0009-workspace-scoped-extensions.md);
    once Phase 3 is implemented it supersedes this interim guarantee.

### Storage and Sidecars

55. Each workspace SHALL retain its own canonical SQLite store.
56. The router SHALL NOT write documents, chunks, embeddings, checkpoints, or
    FTS rows into any shared cross-workspace database.
57. Each workspace's vector-index sidecar SHALL remain derived state for that
    workspace only.
58. The router SHALL NOT treat vector-index sidecars as authoritative storage.
59. Workspace runtime initialization SHALL respect the vector-index behavior
    defined in [SPEC-0012](0012-storage-and-vector-index-interfaces.md).

### Runtime Initialization and Health

60. Workspace registry parsing and cheap validation (id shape, absolute paths,
    config parse) SHALL occur at startup so that obviously invalid workspaces
    are reported as unavailable without blocking the server.
61. Expensive initialization (opening the SQLite store, loading Lua and registry
    extensions) MAY be deferred until the first request that targets a
    workspace. A workspace that fails expensive initialization SHALL be reported
    as `workspace_unavailable` for queries and SHALL appear with an unhealthy
    status in the `workspaces` tool.

### Errors

62. Router errors returned through REST SHALL use the existing JSON error shape.
63. Router errors returned through MCP SHALL be returned as tool errors following
    existing MCP tool-call conventions.
64. Router error codes SHALL include:

   | Code | Meaning |
   |------|---------|
   | `workspace_required` | A request needs an explicit workspace selector. |
   | `unknown_workspace` | No workspace exists with the requested id. |
   | `workspace_disabled` | The requested workspace exists but is disabled. |
   | `workspace_unavailable` | The requested workspace cannot be loaded or queried. |
   | `workspace_id_conflict` | A qualified id conflicts with an explicit workspace field. |
   | `unsupported_workspace_selector` | A selector such as `all` is not valid for the requested operation. |

## Phase 3: Workspace-Scoped Extensions and Request Origin

This section defines how global and workspace-local tools, agents, and prompts
are exposed, resolved by the originating workspace of a request. It applies only
in multi-workspace mode and, once implemented, supersedes the interim guarantee
in requirement 54. The transport binding for request origin (requirement 65) is
defined by [ADR-0023](../adr/0023-request-origin-binding.md) (Proposed) and is
finalized when that ADR is accepted. See
[DESIGN-0009](../design/0009-workspace-scoped-extensions.md).

### Request Origin

65. A session MAY carry a **request origin**: the workspace a client session is
    associated with. Origin SHALL be supplied at the session/transport level (an
    HTTP header or an MCP `initialize` parameter), established at session
    initialization, and SHALL NOT be required per tool call. The server SHALL
    NOT infer origin from server-side filesystem state. The transport binding is
    the `X-Context-Harness-Workspace` HTTP header (primary) with an MCP
    `initialize` parameter as an optional fallback, per
    [ADR-0023](../adr/0023-request-origin-binding.md).
66. An origin value MAY be expressed as a registered workspace id or as an
    absolute root path. A root-path origin SHALL resolve to the registered
    workspace whose `root` equals the supplied path.
67. An origin that does not resolve to an enabled, registered workspace SHALL be
    treated as **unrecognized**. The server SHALL NOT load configuration outside
    the registry to satisfy an origin.
68. When origin is unrecognized or absent, the session SHALL resolve extensions
    with no workspace overrides (built-ins and the global layer only), and
    built-in selection SHALL fall back to requirements 19–20.

### Extension Layers and Resolution

69. Context Harness SHALL recognize two extension layers: a **global layer**
    (root-level shared tools, agents, and prompts under the global config
    directory defined by [SPEC-0013](0013-config-resolution.md)) and a
    **workspace layer** (a workspace's own tools, agents, and prompts).
70. The effective extension set for a session SHALL be the union of the global
    layer and the origin workspace's layer.
71. When the global layer and the workspace layer define an extension with the
    same name, the workspace entry SHALL override (shadow) the global entry for
    that session.
72. A workspace MAY explicitly hide a named global extension. A hidden global
    extension SHALL NOT appear in that session's effective set.
73. Built-in tools (`search`, `get`, `sources`, `workspaces`) SHALL always be
    available and SHALL NOT be shadowed or hidden by any layer.
74. `list_tools` and `list_prompts` SHALL return the session's effective set.
    When a session's origin changes (if permitted), the server SHALL emit the
    corresponding MCP `list_changed` notification.
75. A tool or prompt that is not in the session's effective set SHALL return the
    existing not-found error for tool and prompt calls.

### Execution Scoping

76. A resolved workspace-layer tool or prompt SHALL execute against its owning
    workspace's configuration and store.
77. A resolved global-layer tool or prompt SHALL execute with the session's
    origin workspace as its default workspace context. When origin is
    unrecognized or absent, a global-layer extension that requires a workspace
    context SHALL follow requirements 19–20.
78. Request origin SHALL participate in built-in selector precedence as a tier
    below an explicit `workspace` field (and below a qualified id for `get`) and
    above `[defaults].workspace`. The full precedence SHALL be: qualified id
    (`get`), then explicit `workspace`, then session origin, then
    `[defaults].workspace`, then single-enabled fallback, then
    `workspace_required`.
79. Cross-workspace selectors — an explicit `workspace` field or
    `workspace = "all"` — SHALL remain available regardless of session origin.
    Origin sets the default target, not a restriction.

### Isolation and Compatibility

80. Per-session resolution SHALL NOT expose one workspace's workspace-layer tools
    or prompts to a session whose origin is a different workspace.
81. The redaction requirements in 48–49 SHALL apply to all extension layers.
82. Compatibility (single-workspace) mode SHALL be unaffected by this section.
    Its effective extension set SHALL match the pre-router behavior, preserving
    the additive invariant in requirement 15.

## Security and Trust Model

Context Harness is a **local-first, single-user** tool. The MCP/REST server has
**no authentication** and serves with permissive CORS (`allow_origin(Any)`,
`allow_headers(Any)`). Multi-workspace mode widens the blast radius of that
posture: one endpoint now fronts every registered store, and an explicit
`workspace` selector (and, in Phase 2, `workspace = "all"`) reaches any of them
regardless of session origin (R79). The following constraints define the trust
boundary for Phase 1:

T1. **Loopback bind is the load-bearing control.** The shared server SHALL
    default to `127.0.0.1`. A non-loopback bind exposes every registered
    workspace to other hosts on the network.

T2. **Non-loopback bind is refused in multi-workspace mode** unless the operator
    passes an explicit `--allow-remote` flag. Compatibility mode warns but does
    not refuse (pre-router behavior is preserved). A warning is emitted in both
    modes for any non-loopback bind.

T3. **Redaction is mandatory before exposure.** `sources` and `workspaces`
    output SHALL pass connector configuration through deny-by-default redaction
    (R48/R49) so credentials, tokens, env-expanded values, and URL userinfo do
    not leak across the widened surface.

T4. **Origin is not authorization.** The Phase-3 request origin (R65) sets a
    session's default workspace; it SHALL NOT be used as an access-control
    boundary. Explicit and `all` selectors remain available from any session.

T5. **CORS is intentionally permissive** for local MCP clients in Phase 1. A
    CORS allowlist or a loopback token for multi-workspace mode is a candidate
    hardening (see DESIGN-0008 risks) and is out of scope for Phase 1; operators
    rely on T1/T2 and not exposing the port to untrusted origins.

## Acceptance Criteria

1. Integration tests cover absent-registry behavior and confirm existing
   single-workspace MCP tool calls still work.
2. A golden test asserts that compatibility-mode MCP and REST responses for the
   built-in tools are byte-for-byte identical to the pre-router baseline (the
   additive invariant in requirement 15).
3. Tests confirm that with a registry file present but no `--workspaces` flag,
   the server runs in compatibility mode, and that combining `--workspaces` with
   `--config` or `CTX_CONFIG` is rejected at startup.
4. Tests confirm a registry with exactly one workspace activated with
   `--workspaces` runs in multi-workspace mode and labels results.
5. Tests cover parsing a registry with two enabled workspaces and one disabled
   workspace.
6. Tests cover `search` with an explicit workspace and compare results to the
   same workspace queried through the compatibility path.
7. Tests cover `search` with `workspace = "all"` and assert every result or
   error entry includes workspace identity, including a per-workspace error
   entry for a workspace that exceeds its deadline.
8. Tests cover `get` with an explicit workspace.
9. Tests cover `get` with a qualified document id, and confirm that a raw id
   whose prefix is not a registered workspace (for example `foo:bar`) is treated
   as a raw id rather than a qualified id.
10. Tests cover conflict detection when a qualified id and explicit workspace
    disagree.
11. Tests cover `sources` and `workspaces` output without exposing secret config
    values, including redaction of connector credentials and env-expanded
    values.
12. Tests cover unknown, disabled, and unavailable workspace errors.
13. Tests cover that each workspace uses its own SQLite database path.
14. Tests cover that vector-index sidecar paths are resolved per workspace.
15. (Phase 3) Tests cover that a session with origin = workspace A exposes
    built-ins, the global layer, and A's tools/prompts, and does not expose
    workspace B's local tools.
16. (Phase 3) Tests cover that a global/workspace name collision resolves to the
    workspace entry, and that a workspace hide-list removes a named global
    extension from the session.
17. (Phase 3) Tests cover that an unrecognized or absent origin resolves to
    built-ins and the global layer only, and never loads a config outside the
    registry.
18. (Phase 3) Tests cover that an explicit `workspace` selector and
    `workspace = "all"` work from any session regardless of origin.
19. (Phase 3) Tests cover that compatibility mode is unchanged by Phase 3 (the
    additive invariant).

## Related Documents

- [PRD-0011: Multi-Workspace MCP Router](../prd/0011-multi-workspace-mcp-router.md)
- [DESIGN-0008: Multi-Workspace MCP Router](../design/0008-multi-workspace-mcp-router.md)
- [DESIGN-0009: Workspace-Scoped Extensions and Request Origin](../design/0009-workspace-scoped-extensions.md)
- [SPEC-0012: Storage and Vector Index Interfaces](0012-storage-and-vector-index-interfaces.md)
- [SPEC-0013: Config Resolution and Directory Layout](0013-config-resolution.md)
- [ADR-0009: MCP Streamable HTTP Transport](../adr/0009-mcp-streamable-http-transport.md)
