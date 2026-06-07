# RUNBOOK-0015: Author a Lua Extension

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook covers authoring Lua extensions for Context Harness: connectors, tools, and agents. Use it when creating a new extension from scratch, testing it locally, and publishing it to a registry.

## Prerequisites

- `ctx` CLI binary installed (see [RUNBOOK-0002](0002-build-cli.md))
- Workspace initialized with `config/ctx.toml` (see [RUNBOOK-0010](0010-workspace-init.md))
- Basic familiarity with Lua (see [docs/spec/0008-lua-connectors.md](../spec/0008-lua-connectors.md), [0009-lua-tools.md](../spec/0009-lua-tools.md), [0011-mcp-agents.md](../spec/0011-mcp-agents.md))

## Steps

1. Choose the extension type:

   | Type | Purpose | Entry point |
   |------|----------|-------------|
   | **Connector** | Ingest data into the knowledge base | `connector.scan(config)` → SourceItems |
   | **Tool** | Act on the knowledge base (MCP tool) | `tool.execute(params, context)` → result |
   | **Agent** | Persona with system prompt and tool bindings | `agent.resolve(args, config, context)` → prompt |

2. Create the Lua file with the scaffold command:

   **Connector:**

   ```bash
   ctx connector init my-connector
   ```

   Creates `connectors/my-connector.lua` (or `./connectors/my-connector.lua` in project root).

   **Tool:**

   ```bash
   ctx tool init my-tool
   ```

   Creates `tools/my-tool.lua`.

   **Agent:**

   ```bash
   ctx agent init my-agent
   ```

   Creates `agents/my-agent.lua`.

3. Write the implementation. Each type has a required interface:

   **Connector** — implement `connector.scan(config)` returning an array of items with `source_id`, `body`, and optional `title`, `source_url`, `updated_at`, etc.:

   ```lua
   connector = {
       name = "my-connector",
       version = "0.1.0",
       description = "Ingest items from my API",
   }

   function connector.scan(config)
       local items = {}
       -- Example: local resp = http.get(config.url .. "/api/items", { headers = {...} })
       -- for _, item in ipairs(resp.json.items) do
       --     table.insert(items, {
       --         source_id = item.id,
       --         title = item.title,
       --         body = item.content,
       --         source_url = config.url .. "/items/" .. item.id,
       --         updated_at = item.updated_at,
       --     })
       -- end
       return items
   end
   ```

   **Tool** — implement `tool.execute(params, context)` with `parameters` and return a result table:

   ```lua
   tool = {
       name = "my-tool",
       version = "0.1.0",
       description = "Does something useful",
       parameters = {
           { name = "query", type = "string", required = true, description = "Search query" },
       },
   }

   function tool.execute(params, context)
       local results = context.search(params.query, { limit = 5 })
       return { count = #results, items = results }
   end
   ```

   **Agent** — implement `agent.resolve(args, config, context)` returning `{ system = "...", tools = {...}, messages = {...} }`:

   ```lua
   agent = {
       name = "my-agent",
       description = "Helpful agent with KB context",
       tools = { "search", "get" },
   }

   function agent.resolve(args, config, context)
       local docs = context.search("relevant topic", { limit = 3 })
       local context_text = ""
       for _, r in ipairs(docs) do context_text = context_text .. r.title .. "\n" end
       return {
           system = "You are a helpful assistant. Context:\n" .. context_text,
           tools = agent.tools,
           messages = {},
       }
   end
   ```

4. Add config to `ctx.toml`:

   **Connector:**

   ```toml
   [connectors.script.my-connector]
   path = "connectors/my-connector.lua"
   url = "https://api.example.com"
   api_token = "${MY_API_TOKEN}"
   ```

   **Tool:**

   ```toml
   [tools.script.my-tool]
   path = "tools/my-tool.lua"
   timeout = 30
   ```

   **Agent:**

   ```toml
   [agents.script.my-agent]
   path = "agents/my-agent.lua"
   timeout = 30
   ```

5. Test locally:

   **Connector:**

   ```bash
   ctx --config config/ctx.toml connector test connectors/my-connector.lua --source my-connector
   ```

   Expected: Script loads, `scan()` runs, returns items (or empty list). Then run a real sync:

   ```bash
   ctx --config config/ctx.toml sync script:my-connector
   ```

   **Tool:**

   ```bash
   ctx --config config/ctx.toml tool test tools/my-tool.lua --param query="test" --source my-tool
   ```

   Or start the server and call the tool via HTTP:

   ```bash
   ctx --config config/ctx.toml serve mcp
   # curl -X POST http://127.0.0.1:7331/tools/my-tool -H "Content-Type: application/json" -d '{"query":"test"}'
   ```

   **Agent:**

   ```bash
   ctx --config config/ctx.toml agent test my-agent
   ```

   Or start the server and resolve the prompt via HTTP:

   ```bash
   ctx --config config/ctx.toml serve mcp
   # curl -X POST http://127.0.0.1:7331/agents/my-agent/prompt -H "Content-Type: application/json" -d '{}'
   ```

6. Create the extension manifest for publishing. Registries use a root `registry.toml`; add an entry for your extension. For the community registry at https://github.com/parallax-labs/ctx-registry:

   **Connector** — add under `[connectors.<name>]`:

   ```toml
   [connectors.my-connector]
   description = "Ingest items from my API"
   path = "connectors/my-connector/connector.lua"
   tags = ["api", "custom"]
   required_config = ["url", "api_token"]
   host_apis = ["http", "json", "env"]
   ```

   **Tool** — add under `[tools.<name>]`:

   ```toml
   [tools.my-tool]
   description = "Does something useful with the knowledge base"
   path = "tools/my-tool/tool.lua"
   tags = ["search"]
   host_apis = ["context", "json"]
   ```

   **Agent** — add under `[agents.<name>]`:

   ```toml
   [agents.my-agent]
   description = "Helpful agent with KB context"
   path = "agents/my-agent/agent.lua"
   tags = ["assistant"]
   tools = ["search", "get"]
   ```

7. Publish to a registry. Place the Lua file in the correct directory structure:

   ```
   connectors/my-connector/connector.lua   (or tools/my-tool/tool.lua, agents/my-agent/agent.lua)
   ```

   Add a `config.example.toml` for connectors. Update `registry.toml` with your entry. Commit and push (or open a PR to the community registry):

   ```bash
   git add connectors/my-connector/ registry.toml
   git commit -m "Add my-connector extension"
   git push origin main
   ```

## Verification

- Connector: `ctx connector test` returns items; `ctx sync script:my-connector` completes; `ctx search "term"` returns results.
- Tool: `ctx tool test` returns a result; `ctx serve mcp` exposes the tool in `GET /tools/list`; POST to `/tools/my-tool` returns a valid response.
- Agent: `ctx agent test` prints a resolved prompt; `ctx serve mcp` exposes the agent in `GET /agents/list`; POST to `/agents/my-agent/prompt` returns system prompt and tools.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `connector test` fails with "attempt to index nil" | Missing field in API response or config | Add nil checks; verify config keys match what the script expects |
| `tool test` fails with "context.search not found" | Testing without server; context bridge only available at runtime | Use `ctx serve mcp` and call via HTTP, or ensure test harness provides a mock context |
| `agent test` returns empty system prompt | `agent.resolve` not returning `system` key | Ensure return value has `{ system = "...", tools = {...} }` |
| Extension not discovered from registry | Wrong path in registry.toml or wrong directory structure | Use `connectors/<name>/connector.lua`, `tools/<name>/tool.lua`, `agents/<name>/agent.lua` |
| Lua syntax error on load | Invalid Lua in script | Run `lua connectors/my-connector.lua` to check syntax; fix reported line |
| Script timeout | Long-running HTTP or logic | Increase `timeout` in config; add pagination or reduce scope |

## Related Runbooks

- [RUNBOOK-0014](0014-registry-init-update.md) — Registry Init and Update
- [RUNBOOK-0011](0011-sync-connectors.md) — Sync Connectors

## Related Specs

- [docs/spec/0007-extension-registries.md](../spec/0007-extension-registries.md)
- [docs/spec/0008-lua-connectors.md](../spec/0008-lua-connectors.md)
- [docs/spec/0009-lua-tools.md](../spec/0009-lua-tools.md)
- [docs/spec/0011-mcp-agents.md](../spec/0011-mcp-agents.md)
