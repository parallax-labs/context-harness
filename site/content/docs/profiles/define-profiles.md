+++
title = "Define Profiles"
description = "Create reusable MCP prompts with a role, suggested tools, and optional dynamic context."
weight = 2
aliases = ["/docs/guides/agents/"]
+++

Context Harness has two related but different concepts:

| Concept | Who runs the model? | What Context Harness provides | Use it when |
|---|---|---|---|
| **Profile** | An external client such as Cursor or your own application | A reusable prompt, suggested tool set, and optional pre-resolved context | You already have a chat or agent host and want a consistent project role |
| **Agent** | Context Harness | A bounded model/tool loop with permissions, approvals, durable state, inspection, and recovery | You want Context Harness to own and execute the task |

A profile is not an autonomous agent and does not have a run lifecycle. It is a
named recipe that prepares an external conversation. New configurations use
`profiles.*`, the CLI uses `ctx profile`, and REST integrations use
`/profiles/*`. The historical agent spellings remain deprecated aliases.

An executable agent is a standalone resource under `.ctx/agents`. See
[Build Local Agents](@/docs/agents/build-local-agents.md) for its complete setup.

You may not need a separate profile definition. Single-workspace `ctx serve mcp`
projects each standalone `.ctx/agents/*.toml` resource as a stateless MCP prompt.
That projection reuses the role and tool hints, while the external client—not
the local runtime—owns model calls, tool execution, permissions, and history.

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

### Profile sources

| Mode | Config | Best for |
|------|--------|----------|
| **Standalone projection** | `.ctx/agents/<name>.toml` | Reusing one role for both managed runs and external conversations |
| **Inline TOML** | `[profiles.inline.<name>]` | Static reusable profiles |
| **Lua script** | `[profiles.script.<name>]` | Dynamic context injection, conditional logic |
| **Rust trait** | `impl Profile for MyAgent` | Compiled extensions in custom binaries |

---

### Inline TOML profiles

The simplest way to define a profile—everything lives in `ctx.toml`:

```toml
[profiles.inline.code-reviewer]
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

[profiles.inline.architect]
description = "Answers architecture questions using indexed documentation"
tools = ["search", "get", "sources"]
system_prompt = """
You are a software architect with deep knowledge of this codebase.
Use the search tool to find architecture decision records (ADRs),
design documents, and relevant code patterns. Always cite your sources.
When recommending changes, explain tradeoffs clearly.
"""
```

These profiles appear immediately in `GET /profiles/list` and can be resolved via
`POST /profiles/{name}/prompt`.

---

### Lua scripted profiles

Use a scripted profile for **dynamic context injection**—for example, searching
for relevant runbooks before an external conversation starts:

```toml
[profiles.script.incident-responder]
path = "profiles/incident-responder.lua"
timeout = 30
search_limit = 5
```

```lua
-- profiles/incident-responder.lua

profile = {}

profile.name = "incident-responder"
profile.description = "Helps triage production incidents with relevant runbooks"
profile.tools = { "search", "get", "create_jira_ticket" }

-- Arguments the user can provide
profile.arguments = {
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

function profile.resolve(args, config, context)
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

return profile
```

The `context` bridge provides:

| Function | Description |
|----------|-------------|
| `context.search(query, opts?)` | Search the knowledge base (keyword/semantic/hybrid) |
| `context.get(id)` | Retrieve a full document by UUID |
| `context.sources()` | List all data sources and their status |
| `config` argument | Profile-specific values from `ctx.toml` (env vars expanded) |

---

### HTTP endpoints

#### `GET /profiles/list`

Discover all registered profiles:

```bash
$ curl -s localhost:7331/profiles/list | jq '.profiles[] | {name, description, tools}'
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

#### `POST /profiles/{name}/prompt`

Resolve a profile's prompt (for Lua profiles, this executes `profile.resolve()`):

```bash
$ curl -s localhost:7331/profiles/incident-responder/prompt \
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

Profiles have their own `ctx profile` command group. These commands list,
resolve, or scaffold profiles; they do not execute a model. `ctx agent run`,
`history`, `inspect`, and `resume` belong to executable agents.

```bash
# List profiles, including projections from standalone executable agents
$ ctx profile list
  code-reviewer        Reviews code changes against project conventions   (tools: search, get)        [toml]
  architect            Answers architecture questions using indexed docs   (tools: search, get, sources) [toml]
  incident-responder   Helps triage production incidents with runbooks     (tools: search, get, create_jira_ticket) [lua]

# Resolve a Lua profile with arguments
$ ctx profile test incident-responder --arg service=payments-api --arg severity=P1

Profile: incident-responder
Source: lua (profiles/incident-responder.lua)
Tools: search, get, create_jira_ticket

System prompt (487 chars):
  You are an incident responder for the payments-api service (P1 severity).
  ...

Messages (1):
  [assistant] I'm ready to help with the P1 payments-api incident...

# Scaffold a new Lua profile
$ ctx profile init sre-helper
Created profile: profiles/sre-helper.lua
Add to config:

  [profiles.script.sre-helper]
  path = "profiles/sre-helper.lua"
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
Registered 3 profiles:
  POST /profiles/code-reviewer/prompt — Reviews code changes (toml)
  POST /profiles/architect/prompt — Answers architecture questions (toml)
  POST /profiles/incident-responder/prompt — Helps triage incidents (lua)
MCP server listening on http://127.0.0.1:7331
```

**Step 2:** Resolve a profile's prompt and use it:

Call `POST /profiles/{name}/prompt` to get the system prompt, then use it in the
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
[profiles.inline.code-reviewer]
description = "Reviews code against project conventions and patterns"
tools = ["search", "get"]
system_prompt = """..."""

[profiles.inline.architect]
description = "Answers architecture questions using indexed ADRs and design docs"
tools = ["search", "get", "sources"]
system_prompt = """..."""

# ── Operations ───────────────────────────────────
[profiles.inline.sre-responder]
description = "Helps triage production incidents with runbooks and context"
tools = ["search", "get", "sources"]
system_prompt = """..."""

[profiles.inline.release-manager]
description = "Helps with release planning, changelogs, and deployment"
tools = ["search", "get", "sources"]
system_prompt = """..."""

# ── Knowledge ────────────────────────────────────
[profiles.inline.onboarding]
description = "Guides new engineers through the codebase"
tools = ["search", "get", "sources"]
system_prompt = """..."""

[profiles.inline.tech-writer]
description = "Writes documentation matching project style"
tools = ["search", "get"]
system_prompt = """..."""

# ── Domain Experts (Lua, dynamic) ────────────────
[profiles.script.domain-expert]
path = "profiles/domain-expert.lua"
timeout = 30
search_limit = 10
```

---

### Custom Rust profiles

For compiled profiles in custom harness binaries, implement the `Profile` trait.
The trait resolves a `ProfilePrompt`; it does not execute a model loop:

```rust
use context_harness::{Profile, ProfilePrompt, ProfileArgument};
use context_harness::traits::ToolContext;
use async_trait::async_trait;
use serde_json::Value;
use anyhow::Result;

pub struct DatabaseExpert;

#[async_trait]
impl Profile for DatabaseExpert {
    fn name(&self) -> &str { "db-expert" }
    fn description(&self) -> &str { "Database design and query optimization" }
    fn tools(&self) -> Vec<String> { vec!["search".into(), "get".into()] }

    fn arguments(&self) -> Vec<ProfileArgument> {
        vec![ProfileArgument {
            name: "database".into(),
            description: "Target database name".into(),
            required: false,
        }]
    }

    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<ProfilePrompt> {
        let db = args["database"].as_str().unwrap_or("main");

        // Pre-search for schema documentation
        let results = ctx.search("database schema", None, None, None).await?;
        let context = results.iter()
            .map(|r| format!("- {}", r.title.as_deref().unwrap_or("?")))
            .collect::<Vec<_>>().join("\n");

        Ok(ProfilePrompt {
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
let mut profiles = ProfileRegistry::new();
profiles.register(Box::new(DatabaseExpert));
run_server_with_extensions(config, tools, Arc::new(profiles)).await?;
```

See the [full example](https://github.com/parallax-labs/context-harness/blob/main/examples/custom_harness.rs).

---

### What's next?

- [Build Local Agents](@/docs/agents/build-local-agents.md) — let Context Harness own model execution and durable runs
- [Connect External AI Clients](@/docs/guides/agent-integration.md) — connect Context Harness to Cursor, Claude, and Continue.dev
- [Lua Tools](@/docs/connectors/lua-tools.md) — expose custom actions to external clients
- [Multi-Repo Context](@/docs/guides/multi-repo.md) — index multiple repositories for shared context
- [Deployment](@/docs/reference/deployment.md) — deploy the MCP/profile server in Docker or CI
