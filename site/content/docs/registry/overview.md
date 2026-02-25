+++
title = "Registry Overview"
description = "How extension registries work — Git-backed repos of community connectors, tools, and agents."
weight = 1
+++

Extension registries are Git-backed directories that contain ready-to-use connectors, tools, and agents. Install one with a single command and immediately gain access to dozens of integrations — Jira, Confluence, Slack, RSS, Stack Overflow, and more.

The design follows the same model as [cheat/cheat](https://github.com/cheat/cheat): multiple registry paths with precedence ordering, a read-only community repository, and transparent overrides for local customization.

### What's in a registry?

A registry is a directory (usually a Git repository) containing Lua scripts organized by type:

```
registry.toml                    # manifest describing all extensions
connectors/
  rss/connector.lua              # RSS/Atom feed connector
  jira/connector.lua             # Jira Cloud connector
  slack/connector.lua            # Slack channel history
tools/
  web-fetch/tool.lua             # Fetch and extract web content
  create-jira-ticket/tool.lua    # Create Jira tickets
agents/
  researcher/agent.lua           # KB research agent
  incident-responder/agent.lua   # Incident response agent
```

The `registry.toml` manifest describes each extension with metadata, tags, required configuration, and host API usage:

```toml
[registry]
name = "community"
description = "Official Context Harness community extensions"

[connectors.rss]
description = "Ingest articles from RSS and Atom feeds"
path = "connectors/rss/connector.lua"
tags = ["rss", "atom", "feed", "blog", "news"]
required_config = ["feed_url"]
host_apis = ["http"]
```

### Precedence

When the same extension exists in multiple registries, the highest-precedence source wins:

1. **Explicit `ctx.toml` entries** — always highest priority
2. **`.ctx/` project-local directory** — per-project overrides
3. **Personal registry** — writable, for your customizations
4. **Company registry** — shared across your team (usually read-only)
5. **Community registry** — the official open-source collection

This mirrors how `cheat/cheat` handles cheatpaths: community content provides sensible defaults, and you override at any level without merge conflicts.

### Auto-discovery

**Tools and agents** from registries are automatically available via the MCP server — no config needed. They appear alongside built-in tools when you run `ctx serve mcp`.

**Connectors** require explicit activation because they need credentials. Use `ctx registry add connectors/<name>` to scaffold the config entry, then fill in your credentials.

### The community registry

The official community registry lives at [parallax-labs/ctx-registry](https://github.com/parallax-labs/ctx-registry) and currently includes:

| Type | Count | Examples |
|------|-------|----------|
| **Connectors** | 10 | RSS, Stack Overflow, Dev.to, Hacker News, GitHub Discussions, Jira, Confluence, Notion, Slack, Linear |
| **Tools** | 4 | web-fetch, create-jira-ticket, send-slack-message, create-github-issue |
| **Agents** | 2 | researcher, incident-responder |

Install it with a single command:

```bash
ctx registry init
```
