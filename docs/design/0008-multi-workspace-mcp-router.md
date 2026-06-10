# DESIGN-0008: Multi-Workspace MCP Router

**Status:** Draft
**Date:** 2026-06-09
**Author:** Codex
**Related:** [PRD-0011](../prd/0011-multi-workspace-mcp-router.md),
[SPEC-0014](../spec/0014-multi-workspace-mcp-router.md),
[SPEC-0012](../spec/0012-storage-and-vector-index-interfaces.md),
[SPEC-0013](../spec/0013-config-resolution.md),
[ADR-0009](../adr/0009-mcp-streamable-http-transport.md)

## Context

The current MCP server is built around one effective `Config`. Startup loads a
single config, builds one set of built-in and extension registries, and creates
an `McpBridge` whose tool calls construct a `ToolContext` around that config.
That keeps the single-workspace path simple, but it forces users with multiple
Context Harness stores to run multiple MCP servers or constantly change MCP
client configuration.

The desired product behavior is one MCP endpoint that can route to multiple
workspace stores. The existing storage model already supports the important
boundary: SQLite/FTS5 remains canonical per workspace, and vector indexes are
derived sidecars. The router should sit above those runtimes rather than
combining stores.

The router is additive. The single-config server is the default and must keep
working byte-for-byte; multi-workspace serving is an explicit opt-in. Activation
is therefore tied to a `--workspaces` flag rather than to the mere presence of a
registry file, so an existing single-config deployment cannot be silently
switched into a different response shape.

## Proposal

Introduce a workspace-router layer for MCP and REST serving. The router owns a
set of workspace runtimes and dispatches built-in operations to the selected
runtime.

```text
ctx serve mcp                 # compatibility mode: one resolved Config
ctx serve mcp --workspaces    # multi-workspace mode: load workspaces.toml
  -> build ServerRuntime
       -> WorkspaceRouter
            -> WorkspaceRuntime(context_harness)
            -> WorkspaceRuntime(stack_app)
            -> WorkspaceRuntime(parallax_vault)
  -> expose one Axum server
       -> REST tools endpoints
       -> /mcp streamable HTTP endpoint
```

### WorkspaceRuntime

Each runtime should contain:

- Workspace id.
- Workspace root.
- Resolved `Config`.
- Built-in `ToolContext` or equivalent workspace query context.
- Tool registry loaded for that workspace.
- Agent registry loaded for that workspace.
- Runtime health, including config load errors and store availability.

The runtime should not eagerly sync or mutate the workspace store at server
startup. Startup can validate config shape and construct routing state, but
query operations should continue using the existing store/search code paths.

### WorkspaceRouter

The router should expose methods such as:

```rust
pub struct WorkspaceRouter {
    default_workspace: Option<String>,
    workspaces: HashMap<String, Arc<WorkspaceRuntime>>,
}

impl WorkspaceRouter {
    pub fn resolve(&self, selector: Option<&str>) -> Result<Arc<WorkspaceRuntime>>;
    pub fn resolve_all(&self) -> Vec<Arc<WorkspaceRuntime>>;
    pub async fn search(&self, request: RoutedSearchRequest) -> Result<RoutedSearchResponse>;
    pub async fn get(&self, request: RoutedGetRequest) -> Result<RoutedGetResponse>;
    pub fn sources(&self, request: RoutedSourcesRequest) -> Result<RoutedSourcesResponse>;
}
```

This keeps routing out of `search_documents`, `get_document`, and
`get_sources`. Those functions can remain single-workspace operations that
receive a resolved config.

### MCP Bridge Changes

`McpBridge` should hold `Arc<WorkspaceRouter>` instead of a single
`Arc<Config>` in multi-workspace mode. Built-in tools can be implemented as
router-aware tools:

- `search`
- `get`
- `sources`
- `workspaces`

The single-workspace path can be represented internally as a router with one
workspace. That avoids two server implementations and keeps compatibility
behavior in one place. The *wire contract*, however, is selected by activation
mode rather than by router cardinality: compatibility mode (no `--workspaces`)
emits the existing flat shapes with no extra fields, while multi-workspace mode
(`--workspaces`) emits the workspace-labeled shapes — even when the registry
holds exactly one workspace. See SPEC-0014 requirements 11–17.

### REST Changes

REST endpoints can keep the existing URLs:

- `GET /tools/list`
- `POST /tools/{name}`
- `GET /agents/list`
- `POST /agents/{name}/prompt`
- `GET /health`

For built-in tools, request bodies can accept the same `workspace` field as MCP
tool calls. If the server is in single-workspace mode and no workspace is
provided, behavior should match today's behavior.

A future REST endpoint such as `GET /workspaces/list` can mirror the MCP
`workspaces` tool, but it is not required for the first slice if the tool is
available through `POST /tools/workspaces`.

### Search Response Shape

The initial all-workspace response should prefer clarity over a false global
ranking. A grouped response is easiest to reason about:

```json
{
  "results": [
    {
      "workspace": "context_harness",
      "items": [
        {
          "id": "01J...",
          "qualified_id": "context_harness:01J...",
          "score": 0.83,
          "source": "filesystem:docs",
          "snippet": "..."
        }
      ]
    }
  ],
  "errors": []
}
```

Compatibility mode keeps the existing flat `results` array with no added fields.
The `workspace` and `qualified_id` fields are present only in multi-workspace
mode, and the grouped shape above is the multi-workspace shape for both a single
selected workspace and `all`.

For `all`, workspaces are searched concurrently with a per-workspace deadline so
one slow or locked store cannot stall the response, and `limit` applies per
workspace (there is no global cross-store ranking). Workspaces that fail or time
out appear in `errors` rather than being silently dropped.

### Qualified IDs

The router should parse document ids as either raw ids or qualified ids:

```text
01J...
context_harness:01J...
```

Qualified ids make follow-up `get` calls reliable after all-workspace search.
They also avoid accidental collisions if separate SQLite stores contain the same
UUID-like value.

### Extension Scoping

Workspace-local tools and prompts are the riskiest part because names can
collide. The first phase should guarantee built-in routing only. A later phase
can choose one of these contracts:

- Namespace tool names globally, such as `context_harness.search_release_notes`.
- Keep one global tool name and add a `workspace` parameter to local tools.
- Expose local tools only after selecting a workspace-specific prompt or
  session context.

Until that contract is decided, the router should avoid registering ambiguous
workspace-local extensions globally.

## Alternatives Considered

### One MCP Server Per Workspace

This preserves today's architecture and keeps each runtime simple. It fails the
user workflow: MCP clients become cluttered with ports and duplicate server
configuration, and users must remember which server maps to which project.

### One Merged SQLite Store

A merged store would simplify global search, but it breaks workspace isolation
and conflicts with the current storage contract. It also makes project-specific
connector configuration, checkpoints, and rebuildable vector sidecars harder to
reason about.

### Client-Side Routing

MCP clients could configure many Context Harness servers and decide which one to
call. Most clients do not provide enough routing intelligence, and it pushes
Context Harness workspace semantics into every client.

### Dynamic Port Allocation

The CLI could start one child MCP server per workspace and proxy between them.
This keeps the single-workspace server intact but increases process management
complexity and creates harder failure modes. An in-process router should be
simpler to observe and test.

## Implementation Plan

1. Add the `--workspaces[=<path>]` flag to `ctx serve mcp` and make activation
   explicit. Reject `--workspaces` combined with `--config`/`CTX_CONFIG`.
2. Add workspace registry parsing for `$XDG_CONFIG_HOME/ctx/workspaces.toml`.
3. Add `WorkspaceRuntime` and `WorkspaceRouter` modules. Validate cheaply at
   startup; defer store-open and extension load to first query.
4. Represent single-workspace serve mode as a router containing one workspace,
   but select the wire contract from activation mode, not router cardinality.
5. Add router-aware built-in tools for `search`, `get`, `sources`, and
   `workspaces`.
6. Update `McpBridge` and REST handlers to construct `ToolContext` or equivalent
   routing context from the router.
7. Add qualified-id parsing (qualified only when the prefix matches a registered
   workspace id) and conflict detection.
8. Add all-workspace search with grouped results, bounded concurrency, a
   per-workspace deadline, per-workspace `limit`, and per-workspace error
   reporting.
9. Add a minimal `ctx workspace add/list/remove` command that validates absolute
   paths at write time, so the registry is not hand-edited (mitigates the
   path-drift risk below).
10. Add a golden test that locks the additive invariant: with no `--workspaces`,
    MCP and REST responses are byte-for-byte identical to the pre-router server.
11. Add integration tests with multiple temporary workspace configs and SQLite
    stores.
12. Document MCP examples for default workspace, explicit workspace, and
    all-workspace search (see RUNBOOK-0018).
13. Defer workspace-local extension namespacing to a follow-up design or a later
    section of SPEC-0014 once behavior is locked.

## Acceptance Criteria

- Existing single-workspace MCP tests continue to pass.
- A golden test confirms compatibility-mode responses are byte-for-byte
  identical to the pre-router server (additive invariant).
- With a registry present but no `--workspaces`, the server stays in
  compatibility mode; `--workspaces` with `--config`/`CTX_CONFIG` is rejected.
- A test server can expose multiple independent SQLite stores through one MCP
  endpoint.
- Explicit workspace search matches single-workspace search for that store.
- All-workspace search reports workspace identity for every result and failure.
- `get` supports explicit workspace and qualified ids.
- `sources` and `workspaces` reveal health and routing state without leaking
  secrets.

## Risks

- Cross-workspace result ranking may feel unstable if the first version tries to
  over-merge scores. Grouped responses reduce that risk.
- Loading many workspaces at startup could make the server slow to start. The
  router can record invalid workspaces as unavailable instead of blocking the
  whole server.
- Extension namespacing can become confusing if shipped too early. Built-in
  routing should land first.
- File paths in `workspaces.toml` can drift when projects move. The
  `ctx workspace add` command (plan step 9) should validate paths and make
  registry edits less manual.
- The MCP server already serves with permissive CORS (`allow_origin(Any)`). One
  endpoint now reaching many local stores widens what any web origin can reach on
  localhost. This is pre-existing, but multi-workspace mode increases the blast
  radius and should be noted in deployment guidance.

## Resolved Questions

1. `workspace = "all"` returns grouped results only for v1. A normalized flat
   ranking is explicitly out of scope until there is a real cross-store scoring
   story (raw BM25/cosine are not globally comparable).
2. Workspace runtimes are validated cheaply at startup and initialized lazily on
   first query (store-open and extension load are deferred). The `workspaces`
   tool reports a two-tier health state accordingly.
3. `ctx init` SHALL NOT auto-register the current workspace in
   `workspaces.toml`. Registration is an explicit `ctx workspace add` action,
   keeping multi-workspace behavior opt-in.

## Open Questions

1. Should the registry support workspace aliases in addition to the canonical
   workspace id?
2. What is the final namespacing contract for workspace-local Lua tools, Rust
   tools, and prompts?
