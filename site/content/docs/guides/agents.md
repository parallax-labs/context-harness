+++
title = "MCP Agents"
description = "Define named personas with system prompts, scoped tools, and dynamic context injection."
weight = 6
+++

Agents are **named personas** that combine a system prompt, a scoped set of tools, and optional dynamic context injection. Instead of explaining what you want in every conversation, you define an agent once and activate it by name.

### Why agents?

Without agents, every conversation starts from zero:

```
User: "You are a code reviewer. Search for our coding standards..."
```

With agents, the workflow becomes:

```
User: "Review this PR" (using the code-reviewer agent)
```

The agent pre-configures:
- **System prompt** — grounding the LLM in a specific role
- **Tool scoping** — showing only relevant tools (reduces hallucination)
- **Dynamic context** — pre-fetching docs before the conversation starts

### Three definition modes

| Mode | Config | Best for |
|------|--------|----------|
| **Inline TOML** | `[agents.inline.<name>]` | Static prompts, simple agents |
| **Lua script** | `[agents.script.<name>]` | Dynamic context injection, conditional logic |
| **Rust trait** | `impl Agent for MyAgent` | Compiled extensions in custom binaries |

---

### Inline TOML agents

The simplest way to define an agent — everything in `ctx.toml`:

```toml
[agents.inline.code-reviewer]
description = "Reviews code changes against project conventions"
tools = ["search", "get"]
system_prompt = """
You are a senior code reviewer for this project. When reviewing code:
1. Use the `search` tool to find relevant coding conventions and patterns
2. Use the `get` tool to read full documents when snippets aren't enough
3. Be specific — cite which convention a suggestion relates to
4. Suggest improvements, not just problems

Always ground your feedback in the project's documented standards.
"""

[agents.inline.architect]
description = "Answers architecture questions using indexed documentation"
tools = ["search", "get", "sources"]
system_prompt = """
You are a software architect with deep knowledge of this codebase.
Use the search tool to find architecture decision records (ADRs),
design documents, and relevant code patterns. Always cite your sources.
When recommending changes, explain tradeoffs clearly.
"""
```

These agents appear immediately in `GET /agents/list` and can be resolved via `POST /agents/{name}/prompt`.

---

### Lua scripted agents

For agents that need **dynamic context injection** — pre-searching the knowledge base before the conversation starts:

```toml
[agents.script.incident-responder]
path = "agents/incident-responder.lua"
timeout = 30
search_limit = 5
```

```lua
-- agents/incident-responder.lua

agent = {}

agent.name = "incident-responder"
agent.description = "Helps triage production incidents with relevant runbooks"
agent.tools = { "search", "get", "create_jira_ticket" }

-- Arguments the user can provide
agent.arguments = {
    {
        name = "service",
        description = "The service experiencing the incident",
        required = false,
    },
    {
        name = "severity",
        description = "Incident severity (P1, P2, P3)",
        required = false,
    },
}

function agent.resolve(args, config, context)
    local service = args.service or "unknown"
    local severity = args.severity or "P2"

    -- Pre-search for relevant runbooks
    local results = context.search(
        service .. " incident runbook",
        { mode = "keyword", limit = config.search_limit or 5 }
    )

    -- Fetch full content and inject as context
    local runbook_text = ""
    for _, r in ipairs(results) do
        local doc = context.get(r.id)
        runbook_text = runbook_text .. "\n\n## " .. doc.title .. "\n" .. doc.body
    end

    return {
        system = string.format([[
You are an incident responder for the %s service (%s severity).
You have access to the following runbooks:
%s

Use the search tool for additional context.
Use create_jira_ticket when a tracking ticket is needed.
Be methodical: gather context, identify the issue, recommend actions.
        ]], service, severity, runbook_text),

        -- Inject a starter message
        messages = {
            {
                role = "assistant",
                content = string.format(
                    "I'm ready to help with the %s %s incident. "
                    .. "I've loaded %d relevant runbooks. What's the current situation?",
                    severity, service, #results
                ),
            },
        },
    }
end

return agent
```

The `context` bridge provides:

| Function | Description |
|----------|-------------|
| `context.search(query, opts?)` | Search the knowledge base (keyword/semantic/hybrid) |
| `context.get(id)` | Retrieve a full document by UUID |
| `context.sources()` | List all data sources and their status |
| `context.config` | Tool config from `ctx.toml` (env vars expanded) |

---

### HTTP endpoints

#### `GET /agents/list`

Discover all registered agents:

```bash
$ curl -s localhost:7331/agents/list | jq '.agents[] | {name, description, tools}'
```

```json
{
  "name": "code-reviewer",
  "description": "Reviews code changes against project conventions",
  "tools": ["search", "get"]
}
{
  "name": "incident-responder",
  "description": "Helps triage production incidents with relevant runbooks",
  "tools": ["search", "get", "create_jira_ticket"]
}
```

#### `POST /agents/{name}/prompt`

Resolve an agent's prompt (for Lua agents, this executes `agent.resolve()`):

```bash
$ curl -s localhost:7331/agents/incident-responder/prompt \
    -H "Content-Type: application/json" \
    -d '{"service": "payments-api", "severity": "P1"}' | jq .
```

```json
{
  "system": "You are an incident responder for the payments-api service (P1 severity)...",
  "tools": ["search", "get", "create_jira_ticket"],
  "messages": [
    {
      "role": "assistant",
      "content": "I'm ready to help with the P1 payments-api incident..."
    }
  ]
}
```

| Status | Meaning |
|--------|---------|
| `200` | Success |
| `404` | Agent not found |
| `500` | Lua resolve() failed |
| `408` | Lua resolve() timed out |

---

### CLI commands

```bash
# List all configured agents
$ ctx agent list
  code-reviewer        Reviews code changes against project conventions   (tools: search, get)        [toml]
  architect            Answers architecture questions using indexed docs   (tools: search, get, sources) [toml]
  incident-responder   Helps triage production incidents with runbooks     (tools: search, get, create_jira_ticket) [lua]

# Test a Lua agent with arguments
$ ctx agent test incident-responder --arg service=payments-api --arg severity=P1

Agent: incident-responder
Source: lua (agents/incident-responder.lua)
Tools: search, get, create_jira_ticket

System prompt (487 chars):
  You are an incident responder for the payments-api service (P1 severity).
  ...

Messages (1):
  [assistant] I'm ready to help with the P1 payments-api incident...

# Scaffold a new Lua agent
$ ctx agent init sre-helper
Created: agents/sre-helper.lua
Add to config:

  [agents.script.sre-helper]
  path = "agents/sre-helper.lua"
  timeout = 30
```

---

### Using agents with Cursor

Once your agents are configured and the MCP server is running, you can activate agents in Cursor conversations.

**Step 1:** Start the server with agents:

```bash
$ ctx serve mcp --config ./config/ctx.toml
Registered 6 tools:
  POST /tools/search — Search indexed documents (builtin)
  POST /tools/get — Get document by ID (builtin)
  POST /tools/sources — List data sources (builtin)
Registered 3 agents:
  POST /agents/code-reviewer/prompt — Reviews code changes (toml)
  POST /agents/architect/prompt — Answers architecture questions (toml)
  POST /agents/incident-responder/prompt — Helps triage incidents (lua)
MCP server listening on http://127.0.0.1:7331
```

**Step 2:** Resolve an agent's prompt and use it:

The simplest integration: call `POST /agents/{name}/prompt` to get the system prompt, then use it in your LLM conversation. The agent's `tools` array tells you which Context Harness tools to make available.

**Step 3:** In Cursor, the agent pattern works naturally:

- *"Use the code-reviewer agent to review this PR"* → Cursor resolves the agent, gets the system prompt, and uses the scoped tools
- *"As the architect, how should we restructure the auth module?"*
- *"Assume the incident-responder role for a P1 in the payment service"*

---

### SDLC agent examples

Here's a set of agents that cover the full software development lifecycle:

```toml
# config/ctx.toml

# ── Development ──────────────────────────────────
[agents.inline.code-reviewer]
description = "Reviews code against project conventions and patterns"
tools = ["search", "get"]
system_prompt = """..."""

[agents.inline.architect]
description = "Answers architecture questions using indexed ADRs and design docs"
tools = ["search", "get", "sources"]
system_prompt = """..."""

# ── Operations ───────────────────────────────────
[agents.inline.sre-responder]
description = "Helps triage production incidents with runbooks and context"
tools = ["search", "get", "sources"]
system_prompt = """..."""

[agents.inline.release-manager]
description = "Helps with release planning, changelogs, and deployment"
tools = ["search", "get", "sources"]
system_prompt = """..."""

# ── Knowledge ────────────────────────────────────
[agents.inline.onboarding]
description = "Guides new engineers through the codebase"
tools = ["search", "get", "sources"]
system_prompt = """..."""

[agents.inline.tech-writer]
description = "Writes documentation matching project style"
tools = ["search", "get"]
system_prompt = """..."""

# ── Domain Experts (Lua, dynamic) ────────────────
[agents.script.domain-expert]
path = "agents/domain-expert.lua"
timeout = 30
search_limit = 10
```

---

### Custom Rust agents

For compiled agents in custom harness binaries, implement the `Agent` trait:

```rust
use context_harness::{Agent, AgentPrompt, AgentArgument};
use context_harness::traits::ToolContext;
use async_trait::async_trait;
use serde_json::Value;
use anyhow::Result;

pub struct DatabaseExpert;

#[async_trait]
impl Agent for DatabaseExpert {
    fn name(&self) -> &str { "db-expert" }
    fn description(&self) -> &str { "Database design and query optimization" }
    fn tools(&self) -> Vec<String> { vec!["search".into(), "get".into()] }

    fn arguments(&self) -> Vec<AgentArgument> {
        vec![AgentArgument {
            name: "database".into(),
            description: "Target database name".into(),
            required: false,
        }]
    }

    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<AgentPrompt> {
        let db = args["database"].as_str().unwrap_or("main");

        // Pre-search for schema documentation
        let results = ctx.search("database schema", None, None, None).await?;
        let context = results.iter()
            .map(|r| format!("- {}", r.title.as_deref().unwrap_or("?")))
            .collect::<Vec<_>>().join("\n");

        Ok(AgentPrompt {
            system: format!(
                "You are a database expert for '{}'.\nRelevant docs:\n{}",
                db, context
            ),
            tools: self.tools(),
            messages: vec![],
        })
    }
}
```

Register it in your custom binary:

```rust
let mut agents = AgentRegistry::new();
agents.register(Box::new(DatabaseExpert));
run_server_with_extensions(config, tools, Arc::new(agents)).await?;
```

See the [full example](https://github.com/parallax-labs/context-harness/blob/main/examples/custom_harness.rs).

---

### What's next?

- [Agent Integration](@/docs/guides/agent-integration.md) — connect agents to Cursor, Claude, Continue.dev
- [Lua Tools](@/docs/connectors/lua-tools.md) — give agents custom actions beyond search
- [Multi-Repo Context](@/docs/guides/multi-repo.md) — index multiple repos for cross-project agents
- [Deployment](@/docs/reference/deployment.md) — deploy agents in Docker or CI


