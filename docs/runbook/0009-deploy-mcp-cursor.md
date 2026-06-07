# RUNBOOK-0009: Deploy MCP Server for Cursor

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook configures the Context Harness MCP server for use with Cursor IDE. Use it when connecting Cursor to your local or remote context index, or when setting up multiple projects with different MCP instances.

## Prerequisites

- `ctx` binary installed (see [RUNBOOK-0002](0002-build-cli.md) or [RUNBOOK-0007](0007-deploy-docker.md))
- A config file (`ctx.toml`) with connectors and server bind configured
- Cursor IDE installed

## Steps

1. Install the `ctx` binary. Either build from source or use a pre-built binary.

   ```bash
   cargo install --path crates/context-harness
   ctx --version
   ```

   Expected output (or similar):

   ```
   ctx 0.x.x
   ```

2. Create workspace config. Ensure you have `config/ctx.toml` (or a custom path) with `[server] bind` set. For Cursor, binding to `127.0.0.1:7331` is typical.

   Example `config/ctx.toml` snippet:

   ```toml
   [server]
   bind = "127.0.0.1:7331"
   ```

   Expected: Config file exists and specifies the bind address and port.

3. Initialize and sync the context index (one-time or after config changes).

   ```bash
   cd /path/to/your/project
   ctx init --config config/ctx.toml
   ctx sync all --full --config config/ctx.toml
   ctx embed pending --config config/ctx.toml
   ```

   Expected output (or similar):

   ```
   Initialized database at ./data/ctx.sqlite
   Syncing connectors...
   ...
   ```

4. Start the MCP server. Run in a terminal or as a background process.

   ```bash
   ctx serve mcp --config config/ctx.toml
   ```

   Expected output (or similar):

   ```
   MCP server listening on http://127.0.0.1:7331/mcp
   ```

5. Configure Cursor's MCP settings. Create or edit `~/.cursor/mcp.json` (or `.cursor/mcp.json` in your project root for project-scoped config).

   ```json
   {
     "mcpServers": {
       "context-harness": {
         "url": "http://127.0.0.1:7331/mcp"
       }
     }
   }
   ```

   Expected: JSON file is valid. Cursor reads this on startup or when MCP settings are refreshed.

6. Restart Cursor or reload MCP. Open Cursor Settings → MCP (or use the command palette: "MCP: Refresh") to pick up the new server.

   Expected: Cursor shows `context-harness` in the MCP servers list.

7. Verify in Cursor. Open the MCP panel (Settings → MCP or equivalent). The `context-harness` server should appear as connected. You can invoke tools like `search` and `get` from the AI chat.

   Expected: MCP server is listed and tools are available in chat.

## Multi-project setup with different ports

When running multiple projects, each needs its own MCP server on a different port.

1. Create project-specific configs with different ports, e.g. `config/ctx-project-a.toml`:

   ```toml
   [server]
   bind = "127.0.0.1:7331"
   ```

   And `config/ctx-project-b.toml`:

   ```toml
   [server]
   bind = "127.0.0.1:7332"
   ```

2. Start each server in its project directory:

   ```bash
   # Terminal 1 (project A)
   cd /path/to/project-a
   ctx serve mcp --config config/ctx-project-a.toml

   # Terminal 2 (project B)
   cd /path/to/project-b
   ctx serve mcp --config config/ctx-project-b.toml
   ```

3. Use project-scoped `.cursor/mcp.json` in each project to point at the correct port, or list both in `~/.cursor/mcp.json`:

   ```json
   {
     "mcpServers": {
       "context-harness-a": {
         "url": "http://127.0.0.1:7331/mcp"
       },
       "context-harness-b": {
         "url": "http://127.0.0.1:7332/mcp"
       }
     }
   }
   ```

   Expected: Each project uses its own context index.

## Claude Desktop config

For Claude Desktop (or other MCP clients), add the server to your config. On macOS, edit `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

Expected: Claude Desktop can connect to the MCP server when it is running.

## Verification

- `curl http://127.0.0.1:7331/health` returns `{"status":"ok"}`.
- Cursor MCP panel shows `context-harness` as connected.
- AI chat can use Context Harness tools (e.g., search, get document).

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| Cursor does not show the server | Config not loaded or wrong path | Ensure `~/.cursor/mcp.json` or project `.cursor/mcp.json` exists; restart Cursor; check JSON syntax |
| "Connection refused" or "Failed to connect" | MCP server not running | Start `ctx serve mcp` before opening Cursor; ensure port matches config |
| Wrong project context in Cursor | Multiple servers or wrong URL | Use project-scoped `.cursor/mcp.json` or ensure the correct server URL for the active project |
| Tools not available in chat | Server connected but tools not exposed | Verify `ctx serve mcp` is running; check Cursor MCP docs for tool discovery |
| Port already in use | Another process on 7331 | Change `[server] bind` to a different port (e.g. 7332) and update `mcp.json` |

## Related Runbooks

- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI
- [RUNBOOK-0007](0007-deploy-docker.md) — Deploy with Docker (MCP server in container)
- [RUNBOOK-0008](0008-deploy-systemd.md) — Deploy with systemd (MCP server as service)
