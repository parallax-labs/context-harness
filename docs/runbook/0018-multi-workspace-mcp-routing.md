# RUNBOOK-0018: Serve Multiple Workspaces Over One MCP Endpoint

**Status:** Draft
**Date:** 2026-06-09
**Author:** pjones
**Last Verified:** (not yet — describes planned behavior per SPEC-0014)

> This runbook documents the operator procedure for the multi-workspace MCP
> router defined in [SPEC-0014](../spec/0014-multi-workspace-mcp-router.md) and
> [DESIGN-0008](../design/0008-multi-workspace-mcp-router.md). The feature is not
> yet implemented; mark this runbook **Active** and set **Last Verified** once it
> ships and the steps have been run end-to-end.

## Purpose

Run one long-lived `ctx` MCP server that routes `search`, `get`, and `sources`
to several Context Harness workspaces, so a single MCP client (Cursor, Claude
Desktop, Codex) can reach every project without a server-per-project. Use this
when you work across multiple repositories and want one MCP URL.

Multi-workspace mode is **additive and opt-in**. If you only have one workspace,
keep using [RUNBOOK-0009](0009-deploy-mcp-cursor.md) — nothing here changes that
path.

## Prerequisites

- `ctx` binary installed (see [RUNBOOK-0002](0002-build-cli.md)).
- Each workspace already initialized and synced with its own config and SQLite
  store (see [RUNBOOK-0010](0010-workspace-init.md),
  [RUNBOOK-0011](0011-sync-connectors.md),
  [RUNBOOK-0012](0012-manage-embeddings.md)). The router does not sync or mutate
  stores.
- You know each workspace's absolute root path (and its config path if it is not
  at the default `<root>/.ctx/config.toml`).
- An MCP client that connects to an HTTP MCP endpoint by URL.

## Steps

1. Confirm the `ctx` binary is available.

   ```bash
   ctx --version
   ```

   Expected output (or similar):

   ```
   ctx 0.x.x
   ```

2. Confirm each workspace is healthy on its own (compatibility path) before
   registering it. Run from each workspace root:

   ```bash
   cd /abs/path/to/context-harness
   ctx sources
   ctx search "smoke test" --mode keyword --limit 3
   ```

   Expected: connectors are listed and search returns results. If a workspace
   fails here, fix it before adding it to the registry.

3. Register the workspaces. Prefer the validated CLI over hand-editing:

   ```bash
   ctx workspace add context_harness --root /abs/path/to/context-harness
   ctx workspace add stack_app        --root /abs/path/to/stack-app
   ctx workspace add parallax_vault   --root "/abs/path/to/Parallax Labs"
   ctx workspace list
   ```

   Expected: `ctx workspace list` shows each id, its root, and `enabled = true`.

   The CLI writes `$XDG_CONFIG_HOME/ctx/workspaces.toml` (default
   `~/.config/ctx/workspaces.toml`; overridable with `CTX_CONFIG_DIR`). To edit
   by hand instead, create that file with this shape:

   ```toml
   [defaults]
   workspace = "context_harness"   # used when a request omits `workspace`
   bind = "127.0.0.1:7331"         # shared endpoint for multi-workspace mode

   [workspaces.context_harness]
   root = "/abs/path/to/context-harness"
   # config = "/abs/path/to/context-harness/.ctx/config.toml"   # optional
   enabled = true

   [workspaces.stack_app]
   root = "/abs/path/to/stack-app"
   enabled = true

   [workspaces.parallax_vault]
   root = "/abs/path/to/Parallax Labs"
   enabled = true
   ```

   Notes:
   - `root` and `config` MUST be absolute paths.
   - Omit `config` to resolve the workspace's own `.ctx/config.toml` (global
     config merges in). Set `config` to pin one file as the sole source (no
     global merge). See [SPEC-0013](../spec/0013-config-resolution.md).
   - Set `enabled = false` to keep a workspace listed but reject its queries.

4. Start the server in multi-workspace mode with the explicit opt-in flag.

   ```bash
   ctx serve mcp --workspaces
   ```

   Expected output (or similar):

   ```
   MCP server listening on http://127.0.0.1:7331
     MCP endpoint: http://127.0.0.1:7331/mcp
   ```

   Do **not** pass `--config` or set `CTX_CONFIG` with `--workspaces` — that
   combination is rejected. A bare `ctx serve mcp` (no `--workspaces`) stays in
   single-workspace compatibility mode even if `workspaces.toml` exists.

5. Point your MCP client at the single endpoint URL — unchanged from the
   single-workspace setup. For Cursor (`~/.cursor/mcp.json`) or Claude Desktop:

   ```json
   {
     "mcpServers": {
       "context-harness": {
         "url": "http://127.0.0.1:7331/mcp"
       }
     }
   }
   ```

   Expected: the client connects and lists the built-in tools, including the
   `workspaces` discovery tool.

6. Discover registered workspaces and their health.

   ```bash
   curl -s -X POST http://127.0.0.1:7331/tools/workspaces | jq
   ```

   Expected: each workspace with `id`, `root`, `enabled`, `default`, and health.
   No secret config values appear.

7. Search a specific workspace by id (results are labeled by workspace).

   ```bash
   curl -s -X POST http://127.0.0.1:7331/tools/search \
     -H 'content-type: application/json' \
     -d '{"query":"deployment runbook","workspace":"context_harness","limit":5}' | jq
   ```

   Expected: every item includes `workspace` and `qualified_id`
   (`context_harness:<id>`).

8. Search across all workspaces.

   ```bash
   curl -s -X POST http://127.0.0.1:7331/tools/search \
     -H 'content-type: application/json' \
     -d '{"query":"incident response","workspace":"all","limit":5}' | jq
   ```

   Expected: results grouped by workspace, `limit` applied per workspace, and an
   `errors` array that names any workspace that failed or timed out.

9. Retrieve a document. Use a qualified id (recommended after an `all` search):

   ```bash
   curl -s -X POST http://127.0.0.1:7331/tools/get \
     -H 'content-type: application/json' \
     -d '{"id":"context_harness:01J..."}' | jq
   ```

   Or pass `workspace` plus a raw id. They must agree, or you get
   `workspace_id_conflict`.

## Verification

- `curl http://127.0.0.1:7331/health` returns `{"status":"ok"}`.
- The `workspaces` tool lists every registered workspace with a health status.
- `search` with an explicit `workspace` returns the same results as querying
  that workspace through the compatibility path (RUNBOOK-0009).
- `search` with `workspace = "all"` returns workspace-labeled, grouped results.
- A bare `ctx serve mcp` (no `--workspaces`) still behaves exactly as before
  (additive invariant).

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| Server behaves single-workspace; `workspaces` tool missing | Started without `--workspaces` | Restart with `ctx serve mcp --workspaces`. File presence alone does not activate multi-workspace mode. |
| Startup error: `--workspaces` cannot be combined with `--config`/`CTX_CONFIG` | Both an explicit config and the registry were requested | Drop `--config`/unset `CTX_CONFIG`, or drop `--workspaces`. |
| `workspace_required` error on a request | No `[defaults].workspace` and more than one enabled workspace | Set `[defaults].workspace`, or pass `workspace` on the call. (One enabled workspace is used automatically.) |
| `unknown_workspace` | Id not in the registry | Check `ctx workspace list`; fix the `workspace` value or add the workspace. |
| `workspace_disabled` | `enabled = false` | Set `enabled = true` and restart. |
| `workspace_unavailable` | Bad path, unparseable config, or store cannot open | Check the `workspaces` tool health; verify absolute `root`/`config` and that the SQLite store exists. |
| `workspace_id_conflict` on `get` | `workspace` field disagrees with the qualified id prefix | Send only one, or make them match. |
| Workspace silently missing from registry parse | Relative `root`/`config` or invalid id | Use absolute paths and ids matching `[A-Za-z0-9][A-Za-z0-9_-]*`. |

## Rollback

Stop the server and start it in compatibility mode (`ctx serve mcp` with the
workspace's own `--config`), or delete/rename
`$XDG_CONFIG_HOME/ctx/workspaces.toml`. The registry is a routing-only file; no
workspace store is modified by adding, removing, or deleting it.

## Notes

- Each workspace keeps its own SQLite store and vector-index sidecar; the router
  never merges stores or treats sidecars as authoritative.
- `sources` and `workspaces` redact connector secrets and env-expanded values.
- The MCP server serves with permissive CORS; bind to `127.0.0.1` and do not
  expose it to untrusted origins.

## Related Runbooks

- [RUNBOOK-0009](0009-deploy-mcp-cursor.md) — Deploy MCP Server for Cursor (single workspace)
- [RUNBOOK-0010](0010-workspace-init.md) — Initialize a Workspace
- [RUNBOOK-0011](0011-sync-connectors.md) — Sync Connectors
- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI

See [SPEC-0014](../spec/0014-multi-workspace-mcp-router.md) for the behavioral
contract and [DESIGN-0008](../design/0008-multi-workspace-mcp-router.md) for
background.
