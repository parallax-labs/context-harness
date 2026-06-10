# PRD-0011: Multi-Workspace MCP Router

**Status:** Draft
**Date:** 2026-06-09
**Author:** Codex

## Problem Statement

Context Harness currently exposes one MCP server per effective configuration and
store. This works for a single project, but it becomes awkward when a developer
uses Context Harness across many independent workspaces. Each workspace can have
its own `.ctx/config.toml`, SQLite store, connector set, retrieval tuning,
embedding state, and vector-index sidecar, but MCP clients expect a small number
of configured servers.

Users should be able to run one long-lived MCP server that knows about multiple
Context Harness workspaces and routes search, document retrieval, source status,
tools, and prompts to the right workspace without manually switching client
configuration or running a port per project.

## Target Users

- Developers using Context Harness across multiple local repositories or
  workspaces.
- Users who want one MCP server connected to Cursor, Claude Desktop, Codex, or
  other MCP clients while moving between projects.
- Teams that keep separate workspace stores for isolation, reproducibility, and
  project-specific connector configuration.
- Extension authors who need predictable workspace scoping for MCP tools and
  prompts.

## Goals

1. A user can register at least five local workspaces and expose them through a
   single MCP endpoint.
2. A user can search a specific workspace by stable workspace id without changing
   MCP client configuration.
3. A user can search all registered workspaces from the same MCP tool call and
   see which workspace produced each result.
4. A document returned from search can be retrieved unambiguously through the
   same server.
5. Workspace-specific source status is discoverable through MCP so the user can
   understand which stores are healthy or stale.
6. Existing single-config MCP deployments continue to work without changing
   their client configuration. Multi-workspace behavior is additive and opt-in:
   it is activated by an explicit serve flag, never implicitly by the presence
   of a registry file.

## Non-Goals

- Merging all workspace data into one canonical SQLite database.
- Making vector-index sidecars authoritative across workspaces.
- Syncing or mutating workspace stores through the initial routing layer.
- Replacing existing `--config`, `CTX_CONFIG`, `.ctx/config.toml`, or legacy
  `config/ctx.toml` resolution behavior.
- Providing networked multi-user access control.
- Guaranteeing globally comparable raw search scores across independent
  workspaces.
- Activating multi-workspace mode implicitly from the presence of a registry
  file or from running `ctx serve mcp` without an explicit opt-in.

## User Stories

1. As a developer working in several repositories, I register each repository as
   a Context Harness workspace and run one MCP server for the whole day.
2. As a user asking about a specific project, I choose the `context-harness`
   workspace and receive results only from that workspace's store.
3. As a user who is unsure where a memory lives, I pass `workspace = "all"` and
   receive grouped results labeled by workspace.
4. As a user following a search result, I call `get` with the returned id and the
   server retrieves the document from the correct workspace.
5. As a user debugging stale context, I ask for `sources` and see source health
   grouped by workspace.
6. As an existing single-workspace user, I keep running `ctx serve mcp` and my
   existing MCP client keeps working.

## Requirements

1. The product must support a global registry of known workspaces.
2. The product must expose a single MCP endpoint that can route requests to
   registered workspaces.
3. Built-in search, get, and sources tools must accept an optional workspace
   selector.
4. Search results must identify their originating workspace.
5. Document retrieval must be unambiguous across workspaces.
6. The product must preserve workspace isolation: each workspace keeps its own
   config, SQLite store, connector status, embedding state, and vector-index
   sidecar.
7. The product must support a compatibility mode for existing single-workspace
   configurations, and this mode must be the default. Multi-workspace mode must
   require an explicit opt-in flag and must be rejected when combined with an
   explicit single config (`--config`/`CTX_CONFIG`).
8. The product must provide clear errors for unknown, disabled, or unhealthy
   workspaces.
9. The product should allow users to discover registered workspaces through MCP.

## Phasing

### Phase 1: Built-In Workspace Routing

Add a workspace registry and an explicit `--workspaces` opt-in, route built-in
`search`, `get`, and `sources`, and preserve existing single-config behavior. Ship
a minimal `ctx workspace add/list/remove` so the registry is not hand-edited.

### Phase 2: Cross-Workspace Search

Support `workspace = "all"` with grouped results and stable workspace labels.

### Phase 3: Workspace-Scoped Extensions

Define how Lua tools, Rust tools, and MCP prompts are exposed when multiple
workspaces register extensions with the same name. The approach is a global +
workspace cascade (workspace overrides global) resolved by a session-scoped
**request origin**, so each session sees only the shared globals plus its own
workspace's extensions — which removes the cross-workspace name-collision
problem rather than solving it head-on. See
[DESIGN-0009](../design/0009-workspace-scoped-extensions.md) and SPEC-0014
requirements 65–82.

## Success Criteria

1. A single MCP server can expose at least three test workspaces in integration
   tests.
2. `search` with an explicit workspace returns the same result shape and ranking
   as the single-workspace server for that workspace.
3. `search` across all workspaces returns workspace-labeled results and does not
   hide duplicate document ids from different stores.
4. `get` succeeds for ids returned by single-workspace and all-workspace search.
5. Existing MCP clients configured for the default single-workspace server do not
   need configuration changes.
6. Unknown workspace requests return actionable errors.

## Dependencies and Risks

- Depends on the config resolution model in
  [SPEC-0013](../spec/0013-config-resolution.md).
- Depends on the storage and sidecar boundary in
  [SPEC-0012](../spec/0012-storage-and-vector-index-interfaces.md).
- The MCP bridge currently carries one `Config`, so routing requires a new
  workspace context abstraction.
- Workspace-local tools and prompts can collide by name; the first phase should
  avoid promising a final extension namespacing contract.
- Cross-workspace result ranking can mislead users if raw scores are treated as
  globally comparable.

## Related Documents

- [SPEC-0014: Multi-Workspace MCP Router](../spec/0014-multi-workspace-mcp-router.md)
- [DESIGN-0008: Multi-Workspace MCP Router](../design/0008-multi-workspace-mcp-router.md)
- [SPEC-0012: Storage and Vector Index Interfaces](../spec/0012-storage-and-vector-index-interfaces.md)
- [SPEC-0013: Config Resolution and Directory Layout](../spec/0013-config-resolution.md)
- [ADR-0009: MCP Streamable HTTP Transport](../adr/0009-mcp-streamable-http-transport.md)
