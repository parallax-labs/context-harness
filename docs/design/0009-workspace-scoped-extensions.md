# DESIGN-0009: Workspace-Scoped Extensions and Request Origin

**Status:** Draft
**Date:** 2026-06-09
**Author:** Claude
**Related:** [PRD-0011](../prd/0011-multi-workspace-mcp-router.md) (Phase 3),
[SPEC-0014](../spec/0014-multi-workspace-mcp-router.md),
[DESIGN-0008](0008-multi-workspace-mcp-router.md),
[SPEC-0013](../spec/0013-config-resolution.md),
[ADR-0009](../adr/0009-mcp-streamable-http-transport.md),
[ADR-0013](../adr/0013-git-backed-extension-registries.md),
[ADR-0014](../adr/0014-stateless-agent-architecture.md),
[ADR-0023](../adr/0023-request-origin-binding.md)

## Context

[DESIGN-0008](0008-multi-workspace-mcp-router.md) added a router that dispatches
built-in `search` / `get` / `sources` to the right workspace store. It
deliberately deferred one hard part ([SPEC-0014](../spec/0014-multi-workspace-mcp-router.md)
requirement 54): how workspace-local Lua tools, Rust tools, and prompts are
exposed when multiple workspaces register extensions with the same name. The
interim guarantee is "built-in routing only."

Two product needs motivate closing that gap:

1. **Global shared resources.** A root/global layer should provide tools,
   agents, and prompts available everywhere (e.g. a shared `lint` tool or a
   house "summarize" prompt), without copying them into every workspace.
2. **Per-workspace specialization with override.** Each workspace should expose
   its own tools and prompts, and a workspace should be able to override a
   global resource of the same name — exactly the cascade
   [SPEC-0013](../spec/0013-config-resolution.md) already defines for *config*
   (global defaults < workspace overrides).

The blocker is that, as established in DESIGN-0008, the MCP transport gives the
server **no ambient signal** about which project a request comes from. A
`call_tool` carries `name` + `arguments` and nothing about the client's working
directory. So "expose this workspace's tools based on where the request comes
from" requires a mechanism for the request to **declare its origin**.

This design proposes (a) a two-layer extension model with override, and (b) a
session-scoped **request origin** that selects the workspace layer — and shows
why origin-scoping *dissolves* the namespacing collision problem rather than
solving it head-on.

## Proposal

### Two-layer extension model

```text
effective extensions for a session
  = built-ins                       (always present, never shadowed)
  ∪ global layer                    (root-level shared tools/agents/prompts)
  ∪ origin-workspace layer          (the session's workspace, if any)

precedence on name collision:  workspace  >  global
```

- **Global layer**: tools, agents, and prompts defined at the root/global config
  level (under the global config directory from
  [SPEC-0013](../spec/0013-config-resolution.md)). Shared across all workspaces.
- **Workspace layer**: a workspace's own extensions, loaded by its
  `WorkspaceRuntime`.
- **Override**: when both layers define the same name, the workspace entry
  shadows the global one for that session. A workspace MAY also explicitly *hide*
  a named global extension it does not want.
- **Built-ins are protected**: `search` / `get` / `sources` / `workspaces` are
  always available and cannot be shadowed or hidden, so routing guarantees hold.

This reuses one mental model — "global provides defaults, workspace overrides" —
for both config and extensions, which is the main reason to prefer it.

### Why origin-scoping dissolves namespacing

The deferred plan (SPEC-0014 R53/R54) tried to register *all* workspaces'
extensions into one global MCP namespace, which forces a collision contract
(prefixing, parameters, etc.). Origin-scoping removes the premise: **a given
session only ever resolves one workspace's overrides plus the shared globals.**
Workspace A's `deploy` and Workspace B's `deploy` never coexist in one session,
so they cannot collide. The hard problem disappears.

### Request origin

**Origin is session-scoped and client-declared — not inferred, not per-call.**

```text
client config (per project)                server
  url:  http://127.0.0.1:7331/mcp           │
  origin: "stack_app"  (header / init) ─────┤  on session init:
                                            │    read origin from RequestContext
                                            │    validate against registry allowlist
                                            │    bind session → WorkspaceRuntime
            tool call (no origin arg) ──────▶  resolve effective set for session
```

- Origin is supplied **once** at session initialization via the transport — an
  HTTP header or an MCP `initialize` parameter — set in the MCP client config,
  not produced by the model per call. Most clients (Cursor, Claude Desktop,
  Claude Code) already support per-server `headers` and per-project client
  config, so a project's client declares its own origin naturally.
- Origin MAY be a **workspace id** or an **absolute root path**. A root path is
  matched against registered workspace `root` values.
- **Origin is validated against the registry allowlist.** An origin that does
  not resolve to an enabled, registered workspace is treated as *unrecognized*;
  the server never loads a config outside the registry to satisfy an origin.
  This preserves the allowlist boundary from DESIGN-0008 — a client cannot point
  the server at an arbitrary `.ctx/config.toml` and pull in its secrets.
- When origin is unrecognized or absent, the session resolves **built-ins +
  globals only** (no workspace overrides), and built-in selection falls back to
  the default / single-enabled rules (SPEC-0014 R19–R20).

The grounding hook already exists in the code: `list_tools` and `call_tool`
receive an rmcp `RequestContext` that the current bridge ignores
(`crates/context-harness/src/mcp.rs`, the `_context` parameters). Origin-scoping
is precisely about *reading* that context (the session's header / init param)
instead of discarding it, and building the tool/prompt list per session rather
than from one shared `Arc<ToolRegistry>`.

### Origin in the selector precedence

Origin slots into the existing built-in selector stack as a new low-priority
tier — it sets the *default* target, never a restriction:

```text
qualified id (get)  >  explicit `workspace` arg  >  session origin
                    >  [defaults].workspace  >  single enabled  >  workspace_required
```

So an agent in the `stack_app` session that calls `search` with no selector gets
`stack_app` results; an explicit `workspace: "context_harness"` or
`workspace: "all"` still works from any session. Origin removes per-call ceremony
for the common case without taking away cross-workspace reach.

### Execution scoping

- A resolved **workspace-layer** tool/prompt executes against its owning
  workspace's config and store.
- A resolved **global-layer** tool/prompt executes with the session's origin
  workspace as its default `ToolContext`; with no recognized origin it follows
  the fallback rules above.
- Per-session resolution must not leak one workspace's local extensions into a
  session scoped to a different workspace.

## Alternatives Considered

### Per-call origin argument (model-supplied)

Have each tool call carry a `cwd` / `project_root` argument. Rejected: this
relies on the LLM to populate the right origin on every call. It is unreliable,
invisible to the user, and easy to omit. Origin should be declarative
configuration, not model output.

### Flatten all workspace extensions into one global namespace

The originally-deferred approach: register every workspace's tools globally and
adopt a naming contract (prefixes, `workspace` params). Rejected as the primary
model: it creates an N-way collision surface, bloats `list_tools` with tools the
caller cannot meaningfully use, and pushes workspace semantics into every tool
name. Origin-scoping is strictly simpler. (A global namespace could still be
offered later as an explicit "show everything" mode.)

### One MCP server / endpoint per workspace

Run a process or port per workspace so origin is implicit in the connection.
Rejected for the same reasons as DESIGN-0008: it re-fragments client config and
loses the single-endpoint goal. A path-based route (`/mcp/{workspace}`) is a
lighter variant of this and is considered as a transport option below, but the
header / init-param approach keeps one clean URL.

### Server-side inference from filesystem

Have the server guess the project from server-side state. Rejected: there is no
server-side notion of the client's working directory, and any disk-scanning
inference re-introduces the discovery blast radius rejected in DESIGN-0008.

## Implementation Plan

1. Implement the transport binding for origin decided in
   [ADR-0023](../adr/0023-request-origin-binding.md): the
   `X-Context-Harness-Workspace` HTTP header (primary) with an MCP `initialize`
   parameter fallback.
2. Read origin from the rmcp `RequestContext` in `McpBridge` and bind each
   session to a resolved `WorkspaceRuntime` (or "globals-only").
3. Add a global extension layer: load root-level tools/agents/prompts from the
   global config directory, distinct from workspace registries.
4. Implement per-session effective-set resolution: `built-ins ∪ globals ∪
   workspace`, workspace shadows global, plus a workspace hide-list for globals.
5. Make `list_tools` / `list_prompts` return the per-session effective set and
   emit `list_changed` if origin can change within a session.
6. Add session origin to the built-in selector precedence (below explicit
   selector, above `[defaults].workspace`).
7. Scope execution: workspace-layer extensions run against their workspace;
   global-layer extensions default to the session origin.
8. Enforce isolation: a session never sees another workspace's workspace-layer
   extensions; reject/ignore unrecognized origins per the allowlist.
9. Add REST parity (an origin header for the REST tool endpoints) or document
   that REST callers pass `workspace` explicitly.
10. Tests: per-session resolution, shadow + hide, unrecognized/absent origin
    fallback, cross-workspace selectors still work, compatibility mode unchanged.
11. Graduate the locked behavior into SPEC-0014 Phase 3 (already drafted as
    requirements 65–82) and update PRD-0011 Phase 3.

## Acceptance Criteria

- A session with origin = workspace A exposes built-ins + globals + A's
  tools/prompts, and does **not** expose workspace B's local tools.
- A name collision between the global and workspace layers resolves to the
  workspace entry; a workspace hide-list removes a named global extension from
  that session.
- An unrecognized or absent origin resolves to built-ins + globals only and
  never loads a config outside the registry.
- An explicit `workspace` selector and `workspace = "all"` work from any session
  regardless of origin.
- Compatibility (single-workspace) mode is unchanged (additive invariant,
  SPEC-0014 requirement 15).

## Risks

- **Single-URL erosion.** Per-project origin means each project's client config
  differs by one header/param. This is mild and additive: project-scoped clients
  already differ per project, and a single global client still works with
  explicit selection. Worth stating in deployment docs so it is not a surprise.
- **Session statefulness vs [ADR-0014](../adr/0014-stateless-agent-architecture.md).**
  Binding origin to a session introduces lightweight routing state. It is session
  routing context, not agent state, but the interaction should be reconciled when
  the ADR is written.
- **`list_tools` caching in clients.** Clients may cache the tool list. If origin
  can change mid-session, `list_changed` must fire; simplest v1 is immutable
  origin per session.
- **REST has no session.** The REST endpoints are stateless per request, so REST
  callers likely pass `workspace` explicitly rather than relying on origin. Keep
  the two surfaces consistent in documentation.

## Decided (see [ADR-0023](../adr/0023-request-origin-binding.md), Proposed)

- **Transport binding for origin** — the `X-Context-Harness-Workspace` HTTP
  header is primary, with an MCP `initialize` parameter as an optional fallback;
  the path-based route (`/mcp/{workspace}`) and per-call argument were rejected.
- **Origin is immutable per session** for v1, so no `list_changed` is required
  for origin changes initially.

## Open Questions

1. Should the global extension layer also apply in **compatibility mode**, or
   only in multi-workspace mode? Applying it in compatibility mode risks the
   byte-for-byte additive invariant and needs an explicit opt-in if desired.
2. Should there be an explicit "**show all workspaces' tools**" global-namespace
   mode for power users, layered on top of origin-scoping?
3. What is the on-disk layout for the **global extension layer** (a
   `tools/`/`agents/`/`prompts/` tree under the global config dir, vs. inline in
   the global `config.toml`)?
