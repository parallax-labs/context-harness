# SPEC-0014: Multi-Workspace MCP Router

**Status:** Draft - not yet implemented
**Date:** 2026-06-09
**Scope:** MCP and REST routing across multiple Context Harness workspace
configurations and stores.

## Overview

This spec defines the behavior of a single Context Harness MCP server that can
route requests to multiple registered workspaces. Each workspace retains its own
effective configuration, SQLite store, connector status, embedding state, and
vector-index sidecar. The router is a runtime dispatch layer and SHALL NOT merge
workspace stores into a single canonical database.

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

## Requirements

### Workspace Registry

1. The global workspace registry SHALL be located at
   `$XDG_CONFIG_HOME/ctx/workspaces.toml`, respecting `CTX_CONFIG_DIR` and
   `XDG_CONFIG_HOME` as defined by [SPEC-0013](0013-config-resolution.md).
2. The workspace registry SHALL use this TOML shape:

   ```toml
   [defaults]
   workspace = "context_harness"

   [workspaces.context_harness]
   root = "/absolute/path/to/context-harness"
   config = "/absolute/path/to/context-harness/.ctx/config.toml"
   enabled = true
   ```

3. The `root` field SHALL be required and SHALL be an absolute path.
4. The `config` field MAY be omitted. When omitted, the runtime SHALL resolve
   the workspace configuration from `root` using the workspace-local resolution
   behavior in [SPEC-0013](0013-config-resolution.md).
5. When `config` is present, it SHALL be an absolute path and SHALL be loaded as
   the sole config source, preserving explicit config behavior.
6. The `enabled` field MAY be omitted and SHALL default to `true`.
7. Disabled workspaces SHALL be listed by workspace discovery but SHALL reject
   search, get, sources, tool, and prompt calls.
8. Workspace ids SHALL be unique within the registry.
9. Invalid workspace ids, relative root paths, and relative config paths SHALL
   make the affected workspace invalid and unavailable.
10. The registry MAY be absent. When absent, `ctx serve mcp` SHALL behave as the
    existing single-workspace server.

### Server Modes

11. `ctx serve mcp` SHALL support the existing single-workspace mode.
12. `ctx serve mcp` SHALL support multi-workspace mode when a workspace registry
    exists or an explicit multi-workspace serve option is provided.
13. In single-workspace mode, existing MCP clients SHALL continue to use the same
    endpoint and tool names.
14. In multi-workspace mode, the MCP endpoint SHALL remain `/mcp`.
15. In multi-workspace mode, REST endpoints SHALL remain available on the same
    Axum server.

### Workspace Selection

16. Built-in `search`, `get`, and `sources` tools SHALL accept an optional
    `workspace` field.
17. If `workspace` is omitted, the router SHALL use the default workspace.
18. If no default workspace is configured and more than one enabled workspace is
    available, the router SHALL return a `workspace_required` error.
19. The reserved workspace selector `all` SHALL mean every enabled workspace.
20. `workspace = "all"` SHALL be valid for `search` and `sources`.
21. `workspace = "all"` SHALL NOT be valid for `get` unless the `id` is a
    qualified document id.
22. An unknown workspace id SHALL return an `unknown_workspace` error.
23. A disabled workspace id SHALL return a `workspace_disabled` error.
24. An invalid or unhealthy workspace runtime SHALL return a
    `workspace_unavailable` error.

### Search

25. Single-workspace search SHALL execute against the selected workspace's
    existing search implementation.
26. Single-workspace search SHALL preserve the selected workspace's existing
    search ranking, result fields, retrieval configuration, embedding provider,
    and vector-index fallback behavior.
27. Search responses in multi-workspace mode SHALL include `workspace` on every
    result item.
28. Search responses in multi-workspace mode SHALL include `qualified_id` on
    every result item.
29. For single-workspace search, returned document ids MAY remain unqualified for
    backward compatibility.
30. For `workspace = "all"`, the router SHALL search each enabled workspace
    independently.
31. For `workspace = "all"`, the router SHALL NOT compare raw BM25, cosine, or
    hybrid scores as if they were globally comparable across stores.
32. For `workspace = "all"`, the response SHALL preserve workspace identity for
    every item and SHALL group results by workspace.
33. Workspace search failure SHALL NOT silently remove that workspace from an
    `all` search response. The response SHALL include an error entry for each
    failed workspace.

### Get

34. `get` SHALL retrieve documents from one selected workspace.
35. `get` SHALL accept a qualified document id and route it to the encoded
    workspace.
36. If both an explicit `workspace` field and a qualified document id are
    provided, they SHALL refer to the same workspace.
37. If an explicit `workspace` field conflicts with a qualified document id, the
    router SHALL return `workspace_id_conflict`.
38. `get` SHALL return `not_found` when the selected workspace is available but
    the document id does not exist in that workspace.

### Sources and Workspace Discovery

39. `sources` without `workspace` SHALL use the default workspace in
    single-workspace-compatible mode.
40. `sources` with `workspace = "all"` SHALL return source status grouped by
    workspace.
41. The server SHALL expose a workspace discovery tool named `workspaces`.
42. The `workspaces` tool SHALL return workspace id, root path, enabled status,
    default status, and runtime health.
43. The `workspaces` tool SHALL NOT expose secret values from workspace configs.

### Tools and Prompts

44. Built-in tools SHALL be available in single-workspace and multi-workspace
    modes.
45. Workspace-local Lua tools, Rust tools, and prompts SHALL remain scoped to
    the workspace runtime that loaded them.
46. If multiple workspaces expose local tools or prompts with the same name, the
    router SHALL avoid ambiguous global registration.
47. Until an authoritative namespacing contract is implemented, multi-workspace
    mode SHALL only guarantee routing for built-in tools and workspace
    discovery.

### Storage and Sidecars

48. Each workspace SHALL retain its own canonical SQLite store.
49. The router SHALL NOT write documents, chunks, embeddings, checkpoints, or
    FTS rows into any shared cross-workspace database.
50. Each workspace's vector-index sidecar SHALL remain derived state for that
    workspace only.
51. The router SHALL NOT treat vector-index sidecars as authoritative storage.
52. Workspace runtime initialization SHALL respect the vector-index behavior
    defined in [SPEC-0012](0012-storage-and-vector-index-interfaces.md).

### Errors

53. Router errors returned through REST SHALL use the existing JSON error shape.
54. Router errors returned through MCP SHALL be returned as tool errors following
    existing MCP tool-call conventions.
55. Router error codes SHALL include:

   | Code | Meaning |
   |------|---------|
   | `workspace_required` | A request needs an explicit workspace selector. |
   | `unknown_workspace` | No workspace exists with the requested id. |
   | `workspace_disabled` | The requested workspace exists but is disabled. |
   | `workspace_unavailable` | The requested workspace cannot be loaded or queried. |
   | `workspace_id_conflict` | A qualified id conflicts with an explicit workspace field. |
   | `unsupported_workspace_selector` | A selector such as `all` is not valid for the requested operation. |

## Acceptance Criteria

1. Integration tests cover absent registry behavior and confirm existing
   single-workspace MCP tool calls still work.
2. Tests cover parsing a registry with two enabled workspaces and one disabled
   workspace.
3. Tests cover `search` with an explicit workspace and compare results to the
   same workspace queried through the single-workspace path.
4. Tests cover `search` with `workspace = "all"` and assert every result or
   error entry includes workspace identity.
5. Tests cover `get` with an explicit workspace.
6. Tests cover `get` with a qualified document id.
7. Tests cover conflict detection when a qualified id and explicit workspace
   disagree.
8. Tests cover `sources` and `workspaces` output without exposing secret config
   values.
9. Tests cover unknown, disabled, and unavailable workspace errors.
10. Tests cover that each workspace uses its own SQLite database path.
11. Tests cover that vector-index sidecar paths are resolved per workspace.

## Related Documents

- [PRD-0011: Multi-Workspace MCP Router](../prd/0011-multi-workspace-mcp-router.md)
- [DESIGN-0008: Multi-Workspace MCP Router](../design/0008-multi-workspace-mcp-router.md)
- [SPEC-0012: Storage and Vector Index Interfaces](0012-storage-and-vector-index-interfaces.md)
- [SPEC-0013: Config Resolution and Directory Layout](0013-config-resolution.md)
- [ADR-0009: MCP Streamable HTTP Transport](../adr/0009-mcp-streamable-http-transport.md)
