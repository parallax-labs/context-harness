+++
title = "Cursor & IDE Integration"
description = "Set up Context Harness as workspace-level context for Cursor, VS Code, and JetBrains IDEs."
weight = 5
+++

This guide shows how to turn Context Harness into a personal knowledge layer for your IDE. Every AI interaction — chat, inline completions, code generation — gets grounded in your actual codebase, docs, and internal knowledge.

### The idea

Most AI coding assistants only see the files currently open in your editor. Context Harness gives them access to your *entire* knowledge base — across repos, wikis, and internal tools — so they can answer questions about architecture, find relevant code patterns, and reference documentation you'd otherwise have to search for manually.

```
┌─────────────────┐     ┌─────────────────────┐
│  Cursor / IDE   │────▶│  Context Harness     │
│  Agent          │     │  MCP Server (:7331)  │
│                 │◀────│                      │
│  "How does auth │     │  SQLite + FTS5       │
│   work in our   │     │  + Vector Search     │
│   platform?"    │     │                      │
└─────────────────┘     │  Git repos + Jira    │
                        │  + Confluence + S3   │
                        └─────────────────────┘
```

### Cursor setup

#### Option 1: Workspace-level MCP (recommended)

Create `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://localhost:7331"
    }
  }
}
```

Commit this file so your whole team gets the same context.

#### Option 2: Global MCP

Open **Cursor Settings** → **MCP** → **Add Server**:

| Field | Value |
|-------|-------|
| Name | `context-harness` |
| URL | `http://localhost:7331` |

This makes Context Harness available in *every* Cursor workspace.

#### Option 3: Auto-launch the server

If you don't want to manage the server manually, use the `command` mode:

```json
{
  "mcpServers": {
    "context-harness": {
      "command": "ctx",
      "args": ["serve", "mcp", "--config", "./config/ctx.toml"]
    }
  }
}
```

Cursor will start the server automatically when you open the workspace.

### What Cursor can do with Context Harness

Once connected, Cursor's agent automatically discovers all available tools. Try these prompts:

**Search & understand:**
- *"Search our docs for the authentication flow"*
- *"How does error handling work in the payment service?"*
- *"Find all references to the UserProfile model"*
- *"What changed in the last sprint according to Jira?"*

**Cross-repo context:**
- *"How does the auth service validate tokens? Check the auth-service repo."*
- *"Compare the deployment procedures across platform and infrastructure repos"*

**Write with context:**
- *"Write a new endpoint that follows our existing patterns"* → agent searches for similar endpoints
- *"Generate a migration script based on our schema docs"*
- *"Create an ADR for switching from JWT to session tokens"* → agent searches for existing ADRs

**Custom tool actions:**
- *"Create a Jira ticket for this bug"* → calls Lua tool
- *"Post this deployment summary to the #eng Slack channel"*

### Multi-repo workspace setup

If you work across multiple repos, set up a shared Context Harness instance:

**1. Create a shared context directory:**

```bash
$ mkdir -p ~/ctx-workspace/config
$ cat > ~/ctx-workspace/config/ctx.toml << 'EOF'
[db]
path = "./data/ctx.sqlite"

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536

[retrieval]
final_limit = 12
hybrid_alpha = 0.6

[server]
bind = "127.0.0.1:7331"

[connectors.git]
url = "https://github.com/your-org/main-platform.git"
branch = "main"
include_globs = ["docs/**/*.md", "src/**/*.rs"]
shallow = true
cache_dir = "./data/.cache/platform"

[connectors.script.auth]
path = "connectors/git-repo.lua"
url = "https://github.com/your-org/auth-service.git"
branch = "main"
include_patterns = "src/,docs/,README.md"

[connectors.script.infra]
path = "connectors/git-repo.lua"
url = "https://github.com/your-org/infrastructure.git"
branch = "main"
include_patterns = "docs/,runbooks/"

[connectors.script.jira]
path = "connectors/jira.lua"
url = "https://your-org.atlassian.net"
project = "ENG"
api_token = "${JIRA_API_TOKEN}"
EOF
```

**2. Start the server once:**

```bash
$ cd ~/ctx-workspace
$ ctx init && ctx sync git && ctx sync script:auth && ctx sync script:infra
$ ctx embed pending
$ ctx serve mcp
```

**3. Point all your Cursor workspaces at it:**

In each repo's `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "org-context": {
      "url": "http://localhost:7331"
    }
  }
}
```

Now every Cursor window has access to the full org knowledge base.

### Keep the index fresh

**Manual sync (ad-hoc):**

```bash
$ ctx sync git && ctx sync script:auth && ctx embed pending
```

**Cron job (automatic):**

```bash
# Sync every 2 hours
0 */2 * * * cd ~/ctx-workspace && ctx sync git && ctx sync script:auth && ctx embed pending
```

**Git hook (on push):**

```bash
# .git/hooks/post-push
#!/bin/bash
cd ~/ctx-workspace && ctx sync git --config ./config/ctx.toml &
```

### Claude Desktop

Claude Desktop supports MCP servers through its config file.

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "context-harness": {
      "command": "ctx",
      "args": ["serve", "mcp", "--config", "/Users/you/ctx-workspace/config/ctx.toml"]
    }
  }
}
```

Claude will launch the server automatically when it starts.

### Continue.dev (VS Code / JetBrains)

[Continue.dev](https://continue.dev) supports MCP servers experimentally:

```json
// ~/.continue/config.json
{
  "experimental": {
    "mcpServers": [
      {
        "name": "context-harness",
        "url": "http://localhost:7331"
      }
    ]
  }
}
```

Or use Continue's context provider API for tighter integration:

```json
{
  "contextProviders": [
    {
      "name": "http",
      "params": {
        "url": "http://localhost:7331/tools/search",
        "title": "Knowledge Base",
        "displayTitle": "⚡ Context Harness",
        "description": "Search across all indexed repos and docs"
      }
    }
  ]
}
```

### Windsurf / Codeium

For Windsurf (Codeium's IDE), add Context Harness as an MCP server:

```json
{
  "mcpServers": {
    "context-harness": {
      "serverUrl": "http://localhost:7331"
    }
  }
}
```

### Zed

Zed supports context servers through its extensions system. Use the HTTP API directly:

```json
// settings.json
{
  "context_servers": {
    "context-harness": {
      "url": "http://localhost:7331"
    }
  }
}
```

### Tips for better results

1. **Index ADRs and design docs** — these give the agent architectural context
2. **Index CHANGELOG and commit messages** — helps the agent understand project history
3. **Include README files** — project overviews help agents understand codebases
4. **Use hybrid search** — set `hybrid_alpha = 0.6` for the best mix of keyword + semantic
5. **Keep chunk sizes moderate** — `max_tokens = 700` gives enough context per chunk
6. **Add Lua connectors for tribal knowledge** — Jira, Confluence, Slack threads
7. **Filter by source** — if a question is about infra, the agent can filter with `"source": "script:infra"`

### What's next?

- [Build a RAG Agent](/docs/guides/rag-agent/) — build a standalone Python agent
- [Build a Chatbot](/docs/guides/chatbot/) — build a web-based chat interface
- [Multi-Repo Context](/docs/guides/multi-repo/) — detailed multi-repo configuration
- [Lua Tools](/docs/connectors/lua-tools/) — give agents custom actions

