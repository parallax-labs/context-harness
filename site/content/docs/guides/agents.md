+++
title = "Profiles and Agents"
description = "Understand reusable MCP profiles and executable local agents, when to use each, and how they relate."
weight = 6
+++

Context Harness has two related but different concepts:

| Concept | Who runs the model? | What Context Harness provides | Use it when |
|---|---|---|---|
| **Profile** | An external client such as Cursor or your own application | A reusable prompt, suggested tool set, and optional pre-resolved context | You already have a chat or agent host and want a consistent project role |
| **Agent** | Context Harness | A bounded model/tool loop with permissions, approvals, durable state, inspection, and recovery | You want Context Harness to own and execute the task |

A profile is not an autonomous agent and does not have a run lifecycle. It is a
named recipe that prepares an external conversation. The existing configuration
and API retain their historical `agents.*` and `/agents/*` names for
compatibility, but this guide calls that feature **profiles**.

An executable agent is a standalone resource under `.ctx/agents`. See
[Build Local Agents](@/docs/guides/local-agents.md) for its complete setup.

### Why profiles?

Without a profile, every external conversation must repeat its role and context:

```
User: "You are a code reviewer. Search for our coding standards..."
```

With a profile, the workflow becomes:

```
User: "Review this PR" (using the code-reviewer profile)
```

The profile prepares a launch preset for the external client:

- **System prompt** — grounding the LLM in a specific role
- **Tool suggestions** — telling the external host which tools are relevant
- **Dynamic context** — pre-fetching docs before the conversation starts

Profiles are useful for lightweight integration because they do not require
Context Harness to own model credentials, conversation state, approvals, or the
execution loop. For example, a Cursor extension can resolve a `code-reviewer`
profile, apply its system prompt, enable search/get, and continue the conversation
inside Cursor. Use an executable agent instead when the task must be durable,
auditable, resumable, or governed by Context Harness runtime policy.

Profiles do not retain conversation history. The external client owns any
conversation created from a profile.

### Which one should I use?

- Use a **profile** when Cursor, Claude Desktop, or your application already owns
  the conversation and you want Context Harness to supply a reusable role and
  relevant project context.
- Use an **agent** when you want `ctx` to call the model, execute tools, enforce
  permissions, persist the run, and support inspection or recovery.
- If a new agent run must remember earlier runs, neither mechanism provides that
  automatically today. Store the durable result in a connector source and sync it
  so the next run can retrieve it.

### Three profile definition modes

| Mode | Config | Best for |
|------|--------|----------|
| **Inline TOML** | `[agents.inline.<name>]` | Static reusable profiles |
| **Lua script** | `[agents.script.<name>]` | Dynamic context injection, conditional logic |
| **Rust trait** | `impl Agent for MyAgent` | Compiled extensions in custom binaries |

---

### Inline TOML profiles

The simplest way to define a profile—everything lives in `ctx.toml`:

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

These profiles appear immediately in `GET /agents/list` and can be resolved via
`POST /agents/{name}/prompt`. The endpoint names are retained for compatibility.

---

### Lua scripted profiles

Use a scripted profile for **dynamic context injection**—for example, searching
for relevant runbooks before an external conversation starts:

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

Discover all registered profiles. The response field and route retain their
historical names:

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

Resolve a profile's prompt (for Lua profiles, this executes `agent.resolve()`):

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
| `404` | Profile not found |
| `500` | Lua resolve() failed |
| `408` | Lua resolve() timed out |

---

### CLI commands

The current CLI keeps profiles under `ctx agent` for backward compatibility.
These commands list, resolve, or scaffold profiles; they do not execute a model.
`ctx agent run`, `history`, `inspect`, and `resume` belong to executable agents.

```bash
# List profiles and standalone executable agents
$ ctx agent list
  code-reviewer        Reviews code changes against project conventions   (tools: search, get)        [toml]
  architect            Answers architecture questions using indexed docs   (tools: search, get, sources) [toml]
  incident-responder   Helps triage production incidents with runbooks     (tools: search, get, create_jira_ticket) [lua]

# Resolve a Lua profile with arguments
$ ctx agent test incident-responder --arg service=payments-api --arg severity=P1

Agent: incident-responder
Source: lua (agents/incident-responder.lua)
Tools: search, get, create_jira_ticket

System prompt (487 chars):
  You are an incident responder for the payments-api service (P1 severity).
  ...

Messages (1):
  [assistant] I'm ready to help with the P1 payments-api incident...

# Scaffold a new Lua profile using the compatibility command
$ ctx agent init sre-helper
Created: agents/sre-helper.lua
Add to config:

  [agents.script.sre-helper]
  path = "agents/sre-helper.lua"
  timeout = 30
```

---

### Using profiles with Cursor

Once profiles are configured and the MCP server is running, Cursor or another
client can resolve one when starting a conversation.

**Step 1:** Start the server with profiles:

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

**Step 2:** Resolve a profile's prompt and use it:

Call `POST /agents/{name}/prompt` to get the system prompt, then use it in the
external LLM conversation. The profile's `tools` array tells the client which
Context Harness tools are relevant; the client remains responsible for making
them available and enforcing its own policy.

**Step 3:** In Cursor, the profile pattern works naturally:

- *"Use the code-reviewer profile to review this PR"* → Cursor resolves the profile, gets the system prompt, and uses the suggested tools
- *"As the architect, how should we restructure the auth module?"*
- *"Assume the incident-responder role for a P1 in the payment service"*

---

### SDLC profile examples

Here is a set of reusable profiles for external software-development
conversations:

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

### Custom Rust profiles

For compiled profiles in custom harness binaries, implement the historically
named `Agent` trait. The trait resolves an `AgentPrompt`; it does not execute a
model loop:

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

- [Build Local Agents](@/docs/guides/local-agents.md) — let Context Harness own model execution and durable runs
- [Agent Integration](@/docs/guides/agent-integration.md) — connect profiles to Cursor, Claude, Continue.dev
- [Lua Tools](@/docs/connectors/lua-tools.md) — expose custom actions to external clients
- [Multi-Repo Context](@/docs/guides/multi-repo.md) — index multiple repositories for shared context
- [Deployment](@/docs/reference/deployment.md) — deploy the MCP/profile server in Docker or CI
