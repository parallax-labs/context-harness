# RUNBOOK-0014: Registry Init and Update

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook covers initializing extension registries, browsing and installing extensions, and keeping registries up to date. Use it when setting up Context Harness for the first time, adding new connectors/tools/agents from the community, or refreshing to get the latest extensions.

## Prerequisites

- `ctx` CLI binary installed (see [RUNBOOK-0002](0002-build-cli.md))
- Workspace initialized with `config/ctx.toml` (see [RUNBOOK-0010](0010-workspace-init.md))
- Network access (for cloning the community registry)

## Steps

1. Add registry configuration to `ctx.toml`. Add a `[registries.<name>]` section for each registry you want to use. For the community registry:

   ```toml
   [registries.community]
   url = "https://github.com/parallax-labs/ctx-registry.git"
   branch = "main"
   path = "~/.ctx/registries/community"
   readonly = true
   auto_update = true
   ```

   For a project-local registry:

   ```toml
   [registries.community]
   url = "https://github.com/parallax-labs/ctx-registry.git"
   path = "./registries/community"
   readonly = true
   ```

2. Initialize the registry (clone it to disk). This installs the community registry and its extensions:

   ```bash
   ctx --config config/ctx.toml registry init
   ```

   Expected output (or similar):

   ```
   Cloning community extension registry...
   Installed: 6 connectors, 3 tools, 2 agents
   Added [registries.community] to config/ctx.toml
   Run `ctx registry list` to see available extensions.
   ```

   If the registry config already exists, use `ctx registry install` to clone configured registries that are not yet on disk.

3. Browse available extensions:

   ```bash
   ctx --config config/ctx.toml registry list
   ```

   Expected output (or similar):

   ```
   Registries:
     community — ~/.ctx/registries/community (git) [readonly]
       6 connectors, 3 tools, 2 agents

   Available extensions:
     connectors: jira, confluence, github-issues, ...
     tools: summarize, related-docs, ...
     agents: runbook, code-reviewer, ...
   ```

4. Search for specific extensions by name, description, or tags:

   ```bash
   ctx --config config/ctx.toml registry search atlassian
   ```

   Expected output (or similar):

   ```
   Found 2 extensions matching 'atlassian':
     connectors/jira — Index Jira issues [atlassian, pm] (from: community)
     connectors/confluence — Index Confluence spaces [atlassian, docs] (from: community)
   ```

5. Install (activate) an extension. For connectors, this scaffolds a config entry in `ctx.toml`; tools and agents are auto-discovered at server startup but can also be explicitly configured:

   ```bash
   ctx --config config/ctx.toml registry add connectors/jira
   ```

   Expected output (or similar):

   ```
   Added [connectors.script.jira] to config/ctx.toml
   Edit config/ctx.toml to set: url, project_key, api_token
   ```

   Edit `config/ctx.toml` to fill in credentials (e.g., `url`, `api_token`, `project_key` for Jira). Use `${ENV_VAR}` for secrets.

6. Update the registry to get the latest extensions:

   ```bash
   ctx --config config/ctx.toml registry update
   ```

   Expected output (or similar):

   ```
   Updating community...
     Updated successfully.
   ```

   To update a specific registry: `ctx registry update community`.

7. Verify the installed extension works:

   - **Connectors:** Run sync, then search:

     ```bash
     ctx --config config/ctx.toml sync script:jira
     ctx --config config/ctx.toml search "sprint planning"
     ```

   - **Tools/Agents:** Start the MCP server and confirm they appear:

     ```bash
     ctx --config config/ctx.toml serve mcp
     # In another terminal: curl http://127.0.0.1:7331/tools/list
     # Or: curl http://127.0.0.1:7331/agents/list
     ```

## Verification

- `ctx registry list` shows the registry and its extension counts.
- For connectors: `ctx sync script:<name>` completes without error; `ctx sources` shows the connector as OK.
- For tools/agents: `ctx serve mcp` starts; `GET /tools/list` or `GET /agents/list` includes the extension.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `registry init` fails with "no registries configured" | No `[registries.*]` in ctx.toml | Add registry config (step 1) and re-run |
| `registry install` fails with clone error | Invalid URL, no network, or auth required | Verify URL; for private repos, use SSH URL and ensure SSH key is configured |
| `registry add connectors/jira` reports "not found" | Extension not in any configured registry | Run `ctx registry update`; verify extension exists with `ctx registry search jira` |
| Connector sync fails after `registry add` | Missing or invalid credentials in ctx.toml | Edit config to set `url`, `api_token`, `project_key`; use `${VAR}` for env expansion |
| Tools/agents not in `/tools/list` or `/agents/list` | Server not restarted after registry update | Restart `ctx serve mcp`; registry extensions are loaded at startup |
| `registry update` skips a registry | Uncommitted changes in registry directory | Commit or stash changes in the registry path, or use a fresh clone |

## Related Runbooks

- [RUNBOOK-0010](0010-workspace-init.md) — Initialize a Workspace
- [RUNBOOK-0011](0011-sync-connectors.md) — Sync Connectors
- [RUNBOOK-0015](0015-author-lua-extension.md) — Author a Lua Extension
