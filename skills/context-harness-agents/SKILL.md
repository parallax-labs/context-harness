---
name: context-harness-agents
description: Set up and configure Context Harness standalone local agents, declarative tool bindings, and reusable agent workflows. Use when creating, validating, running, or debugging project agents; do not use for legacy MCP prompt-only personas unless migrating them.
---

# Context Harness Agents

Build agents around the user's project and task, not around a predetermined
knowledge-base shape. Preserve existing configuration and keep every tool at the
narrowest authority that satisfies the requested workflow.

## Workflow

1. Confirm `ctx --version`, the workspace root, and the effective config path.
   Respect `--config` or `CTX_CONFIG` when the user already uses one.
2. Inspect existing `[models.*]`, `[connectors.*]`, `.ctx/agents`, and
   `.ctx/tools` before editing. Never replace an existing config wholesale.
3. For an empty workspace, offer the deterministic initializer:

   ```sh
   python3 scripts/bootstrap_workspace.py \
     --workspace . \
     --agent project-researcher \
     --content-root docs \
     --source project \
     --provider-model gpt-5-mini
   ```

   Run it from this skill directory or use its absolute path. It creates files
   only when none of its targets already exist.
4. For an existing workspace, make minimal edits using the schemas in
   [references/resources.md](references/resources.md). Ask for a model ID only
   when no usable model alias exists; never invent credentials or store a key in
   TOML.
5. Prefer source-scoped `builtin.retrieval.search` and
   `builtin.retrieval.get` bindings for retrieval agents. Add
   `builtin.scoped_file_read` only when the agent needs unindexed file access.
   Use compatibility tools `search` and `get` when scoping to one connector is
   not desired.
6. Give the agent only the public binding names it needs. Start with read-only
   permissions. Add write, process, network, external-side-effect, or delegation
   capabilities only when the requested workflow requires them, and retain the
   normal approval path.
7. Validate declarations before initialization or provider calls:

   ```sh
   ctx agent validate
   ctx tool bindings validate
   ctx agent show <agent> --json
   ctx tool bindings list --json
   ```

8. Initialize and sync only after static validation succeeds:

   ```sh
   ctx init
   ctx sync <connector-type>:<source>
   ctx agent run <agent> "Describe the task"
   ```

9. Report the created paths, effective model alias, connector source ID, public
   tools, validation output, and the exact run command. If a live run is not
   possible, distinguish missing credentials, inaccessible model IDs, empty
   indexes, policy denial, and provider rate limits.

## Agent Design

- Write a durable role and decision policy in `prompt.system`; put the immediate
  objective in the run input.
- Tell retrieval agents when to search, when to fetch a full document, and how
  to cite evidence. Do not hard-code a wiki, vendor, or business domain unless
  the user requests it.
- Keep `max_turns` and `timeout_seconds` proportional to the task. A useful
  starting point is 8 turns and 180 seconds.
- Use delegation only for genuinely separable roles. Keep child authority within
  the parent's authority and declare allowed targets explicitly.
- Treat tool resources as policy-bearing bindings, not executable plugins.
  Compiled, Lua, and MCP implementations must already be registered or configured
  by a trusted host.

## Safety and Recovery

- Static list/show/validate commands must not resolve secrets, start MCP servers,
  evaluate Lua modules, open the database, call a model, or access the network.
- Store secret references as environment-variable names only. Never echo values
  into inspection output, history, or generated files.
- Changed agent or binding identities can invalidate resume. Inspect history
  before editing resources for an interrupted run.
- MCP-backed runs are non-resumable. Lua cancellation is cooperative at VM
  instruction boundaries; blocking host APIs retain their own timeouts.

For command behavior and troubleshooting, read
[references/operations.md](references/operations.md).
