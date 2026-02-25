# MCP Agents — Design & Specification

This document specifies the agent system for Context Harness. Agents are
named personas that combine a system prompt, scoped tools, and optional
dynamic context injection — enabling "assume a role" workflows in Cursor,
Claude Desktop, and other MCP clients.

**Status:** Implemented  
**Author:** Parker Jones  
**Created:** 2026-02-22  
**Depends on:** `traits.rs` (Tool/ToolRegistry), `server.rs` (HTTP server)

---

## 1. Motivation

Context Harness currently provides **tools** (search, get, sources) and
lets agents interact with the knowledge base. But every conversation
starts from zero — the user must explain what they want the AI to do and
which tools matter.

Agents solve this by defining **reusable personas** that:

- Set a **system prompt** — grounding the LLM in a specific role
- Scope **available tools** — showing only relevant tools to reduce noise
- Inject **dynamic context** — pre-fetching relevant docs before the
  conversation starts (RAG-priming)
- Work across **multiple MCP clients** — Cursor, Claude, OpenClaw, etc.

### Use Cases

| Agent | Role | Tools | Dynamic Context |
|-------|------|-------|-----------------|
| `code-reviewer` | Reviews PRs against team conventions | search, get | Pre-searches for coding standards |
| `architect` | Answers architecture questions | search, get, sources | Injects relevant ADRs |
| `onboarding` | Guides new team members | search, get | Pre-searches for onboarding docs |
| `incident-responder` | Helps triage production issues | search, get, create_jira_ticket | Injects runbooks |
| `writer` | Helps write docs matching project style | search, get | Pre-searches for style guide |

### Why Not Just System Prompts?

A plain system prompt doesn't:
- Scope which tools are visible (reducing hallucination)
- Pre-fetch knowledge base context at conversation start
- Run Lua logic to dynamically build the prompt
- Work portably across MCP clients

---

## 2. Architecture Overview

```
ctx.toml / agents/*.lua          HTTP Server / MCP
┌────────────────────┐           ┌─────────────────────────────┐
│ [agents.reviewer]  │           │ GET  /agents/list           │
│ system_prompt = .. │──────────▶│ POST /agents/{name}/prompt  │
│ tools = [search..] │           │ GET  /prompts/list (MCP)    │
│                    │           │ POST /prompts/get  (MCP)    │
│ [agents.architect] │           └─────────────────────────────┘
│ path = "agents/..  │                       │
│ .lua"              │                       ▼
└────────────────────┘           ┌─────────────────────────────┐
                                 │ AgentRegistry               │
                                 │  .list() → Vec<AgentInfo>   │
                                 │  .resolve(name, args)       │
                                 │    → AgentPrompt            │
                                 └─────────────────────────────┘
                                           │
                                           ▼
                                 ┌─────────────────────────────┐
                                 │ AgentPrompt                 │
                                 │  .system: String            │
                                 │  .tools: Vec<String>        │
                                 │  .context_messages:         │
                                 │    Vec<Message>             │
                                 └─────────────────────────────┘
```

### Key Design Decisions

1. **No chat completion endpoint.** Cursor/Claude handles inference.
   The server provides the persona (prompt + context) and tools. The
   LLM does the reasoning.

2. **Two definition modes**: inline TOML (simple) and Lua scripts
   (dynamic). Same pattern as connectors and tools.

3. **MCP-compatible prompts.** Agents are exposed via the MCP
   `prompts/list` and `prompts/get` methods for native client support.

4. **HTTP REST fallback.** Custom clients can use `GET /agents/list`
   and `POST /agents/{name}/prompt` for non-MCP integrations.

5. **Agents are stateless.** Each prompt resolution is independent —
   no sessions, no chat history. The client manages conversation state.

---

## 3. Configuration

### 3.1 Inline TOML Agents (Static)

For agents with fixed system prompts and tool lists:

```toml
[agents.code-reviewer]
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

[agents.architect]
description = "Answers architecture questions using indexed documentation"
tools = ["search", "get", "sources"]
system_prompt = """
You are a software architect with deep knowledge of this codebase.
Use the search tool to find architecture decision records (ADRs),
design documents, and relevant code patterns. Always cite your sources.
When recommending changes, explain tradeoffs clearly.
"""
```

### 3.2 Lua Script Agents (Dynamic)

For agents that need dynamic system prompts, context injection, or
conditional tool scoping:

```toml
[agents.script.incident-responder]
path = "agents/incident-responder.lua"
timeout = 30
# Extra config keys passed to agent.resolve(args, config, context)
search_limit = 5
priority_sources = ["runbooks", "playbooks"]
```

#### Lua Agent Script Interface

```lua
-- agents/incident-responder.lua

agent = {}

-- Required: metadata for discovery
agent.name = "incident-responder"
agent.description = "Helps triage production incidents with relevant runbooks"

-- Required: list of tools this agent should expose
agent.tools = { "search", "get", "create_jira_ticket" }

-- Optional: arguments the user can provide when selecting the agent
agent.arguments = {
    {
        name = "service",
        description = "The service experiencing the incident",
        required = false
    },
    {
        name = "severity",
        description = "Incident severity (P1, P2, P3)",
        required = false
    }
}

-- Required: resolve the prompt for a conversation
-- args: user-provided argument values
-- config: values from ctx.toml agent config
-- context: { search = fn, get = fn, sources = fn }
function agent.resolve(args, config, context)
    local service = args.service or "unknown"
    local severity = args.severity or "P2"

    -- Dynamic context: pre-search for relevant runbooks
    local runbooks = context.search({
        query = service .. " incident runbook",
        mode = "keyword",
        limit = config.search_limit or 5,
        filters = { source = "runbooks" }
    })

    -- Build context messages from search results
    local context_docs = ""
    for _, result in ipairs(runbooks) do
        local doc = context.get(result.id)
        context_docs = context_docs .. "\n\n---\n## " .. (doc.title or "Untitled") .. "\n" .. doc.body
    end

    -- Return the resolved prompt
    return {
        system = string.format([[
You are an incident responder for the %s service (%s severity).

Your job is to help triage and resolve the incident. You have access to
the following runbooks and documentation:

%s

Use the `search` tool to find additional relevant documentation.
Use `create_jira_ticket` to create tracking tickets when needed.
Be methodical: gather context, identify the issue, recommend actions.
        ]], service, severity, context_docs),

        -- Optional: additional messages to inject at conversation start
        messages = {
            {
                role = "assistant",
                content = string.format(
                    "I'm ready to help with the %s %s incident. I've loaded %d relevant runbooks. What's the current situation?",
                    severity, service, #runbooks
                )
            }
        }
    }
end

return agent
```

### 3.3 Rust Trait Agents

For compiled agents in custom harness binaries:

```rust
use context_harness::traits::{Agent, AgentPrompt, AgentArgument, ToolContext};
use async_trait::async_trait;
use serde_json::Value;

pub struct DatabaseExpert;

#[async_trait]
impl Agent for DatabaseExpert {
    fn name(&self) -> &str { "db-expert" }

    fn description(&self) -> &str {
        "Answers database design and query optimization questions"
    }

    fn tools(&self) -> Vec<String> {
        vec!["search".into(), "get".into(), "run_query".into()]
    }

    fn arguments(&self) -> Vec<AgentArgument> {
        vec![AgentArgument {
            name: "database".into(),
            description: "Target database name".into(),
            required: false,
        }]
    }

    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<AgentPrompt> {
        let db = args["database"].as_str().unwrap_or("main");

        // Pre-fetch schema documentation
        let schemas = ctx.search("database schema", SearchOptions {
            mode: Some("keyword".into()),
            limit: Some(5),
            source: Some(format!("schemas:{}", db)),
            ..Default::default()
        }).await?;

        let context = schemas.iter()
            .map(|r| format!("- {} (score: {:.2})", r.title.as_deref().unwrap_or("?"), r.score))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(AgentPrompt {
            system: format!(
                "You are a database expert for the '{}' database.\n\
                 Relevant schema docs:\n{}\n\n\
                 Use search to find more context. Use run_query to execute queries.",
                db, context
            ),
            tools: self.tools(),
            messages: vec![],
        })
    }
}
```

---

## 4. Data Model

### 4.1 AgentInfo (Discovery)

Returned by `GET /agents/list` and `prompts/list`:

```rust
pub struct AgentInfo {
    /// Unique name (used as URL path parameter).
    pub name: String,
    /// One-line description for agent discovery.
    pub description: String,
    /// Tools this agent uses.
    pub tools: Vec<String>,
    /// Source: "toml", "lua", or "rust".
    pub source: String,
    /// Optional arguments the agent accepts.
    pub arguments: Vec<AgentArgument>,
}
```

### 4.2 AgentArgument

```rust
pub struct AgentArgument {
    /// Argument name.
    pub name: String,
    /// Description (shown to user in MCP prompt selection UI).
    pub description: String,
    /// Whether this argument is required.
    pub required: bool,
}
```

### 4.3 AgentPrompt (Resolved)

Returned by `POST /agents/{name}/prompt` and `prompts/get`:

```rust
pub struct AgentPrompt {
    /// The system prompt text.
    pub system: String,
    /// Which tools should be visible for this agent.
    pub tools: Vec<String>,
    /// Optional additional messages to inject (e.g., pre-fetched context).
    pub messages: Vec<PromptMessage>,
}
```

### 4.4 PromptMessage

```rust
pub struct PromptMessage {
    /// Message role: "user", "assistant", or "system".
    pub role: String,
    /// Message content.
    pub content: String,
}
```

---

## 5. Agent Trait

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    /// Returns the agent's unique name (URL-safe).
    fn name(&self) -> &str;

    /// Returns a one-line description for discovery.
    fn description(&self) -> &str;

    /// Returns the list of tool names this agent exposes.
    fn tools(&self) -> Vec<String>;

    /// Returns the arguments this agent accepts (may be empty).
    fn arguments(&self) -> Vec<AgentArgument> {
        vec![]
    }

    /// Resolves the agent's prompt, optionally using the ToolContext
    /// for dynamic context injection (e.g., pre-searching the KB).
    async fn resolve(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<AgentPrompt>;
}
```

---

## 6. AgentRegistry

```rust
pub struct AgentRegistry {
    agents: Vec<Box<dyn Agent>>,
}

impl AgentRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, agent: Box<dyn Agent>);
    pub fn list(&self) -> Vec<&dyn Agent>;
    pub fn find(&self, name: &str) -> Option<&dyn Agent>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;

    /// Build a registry from config, loading TOML and Lua agents.
    pub fn from_config(config: &Config) -> Result<Self>;
}
```

### 6.1 Built-in Adapter Types

| Type | Source | Struct |
|------|--------|--------|
| TOML inline | `[agents.<name>]` in ctx.toml | `TomlAgent` |
| Lua script | `[agents.script.<name>]` in ctx.toml | `LuaAgentAdapter` |
| Custom Rust | `registry.register(Box::new(...))` | User-defined |

---

## 7. HTTP Endpoints

### 7.1 `GET /agents/list`

List all registered agents.

**Response:**

```json
{
  "agents": [
    {
      "name": "code-reviewer",
      "description": "Reviews code changes against project conventions",
      "tools": ["search", "get"],
      "source": "toml",
      "arguments": []
    },
    {
      "name": "incident-responder",
      "description": "Helps triage production incidents with relevant runbooks",
      "tools": ["search", "get", "create_jira_ticket"],
      "source": "lua",
      "arguments": [
        { "name": "service", "description": "The service experiencing the incident", "required": false },
        { "name": "severity", "description": "Incident severity (P1, P2, P3)", "required": false }
      ]
    }
  ]
}
```

### 7.2 `POST /agents/{name}/prompt`

Resolve an agent's prompt. For Lua agents, this executes `agent.resolve()`
which may call `context.search()` and `context.get()`.

**Request:**

```json
{
  "arguments": {
    "service": "payments-api",
    "severity": "P1"
  }
}
```

**Response:**

```json
{
  "result": {
    "system": "You are an incident responder for the payments-api service (P1 severity)...",
    "tools": ["search", "get", "create_jira_ticket"],
    "messages": [
      {
        "role": "assistant",
        "content": "I'm ready to help with the P1 payments-api incident. I've loaded 3 relevant runbooks. What's the current situation?"
      }
    ]
  }
}
```

### 7.3 Error Responses

| Status | Code | When |
|--------|------|------|
| 404 | `not_found` | Agent name not registered |
| 400 | `bad_request` | Missing required argument |
| 500 | `agent_error` | Lua resolve() failed |
| 408 | `timeout` | Lua resolve() exceeded timeout |

---

## 8. MCP Prompt Protocol Mapping

For MCP clients that support the prompts capability, agents are exposed
as MCP prompts via JSON-RPC over the existing transport.

### 8.1 `prompts/list`

Maps directly to `AgentRegistry::list()`:

```json
{
  "prompts": [
    {
      "name": "code-reviewer",
      "description": "Reviews code changes against project conventions",
      "arguments": []
    },
    {
      "name": "incident-responder",
      "description": "Helps triage production incidents with relevant runbooks",
      "arguments": [
        { "name": "service", "description": "...", "required": false },
        { "name": "severity", "description": "...", "required": false }
      ]
    }
  ]
}
```

### 8.2 `prompts/get`

Maps to `Agent::resolve()`. Returns messages in the MCP format:

```json
{
  "description": "Helps triage production incidents with relevant runbooks",
  "messages": [
    {
      "role": "system",
      "content": { "type": "text", "text": "You are an incident responder..." }
    },
    {
      "role": "assistant",
      "content": { "type": "text", "text": "I'm ready to help..." }
    }
  ]
}
```

---

## 9. CLI Commands

### 9.1 `ctx agent list`

Lists all configured agents with descriptions and tool counts.

```
$ ctx agent list
  code-reviewer        Reviews code changes against project conventions   (tools: search, get)
  architect            Answers architecture questions using indexed docs   (tools: search, get, sources)
  incident-responder   Helps triage production incidents with runbooks     (tools: search, get, create_jira_ticket)  [lua]
```

### 9.2 `ctx agent test <name>`

Resolves an agent prompt and prints the result (useful for debugging
Lua agents).

```
$ ctx agent test incident-responder --arg service=payments-api --arg severity=P1

Agent: incident-responder
Source: lua (agents/incident-responder.lua)
Tools: search, get, create_jira_ticket

System prompt (487 chars):
  You are an incident responder for the payments-api service (P1 severity).
  ...

Messages (1):
  [assistant] I'm ready to help with the P1 payments-api incident...

Resolved in 142ms (3 search queries, 2 document fetches)
```

### 9.3 `ctx agent init <name>`

Scaffolds a new Lua agent script.

```
$ ctx agent init sre-helper
Created: agents/sre-helper.lua
Add to config:

  [agents.script.sre-helper]
  path = "agents/sre-helper.lua"
  timeout = 30
```

---

## 10. Tool Scoping

When an agent is active, the MCP client should ideally only see the
tools listed in `agent.tools`. Context Harness supports this through:

### 10.1 Response-Level Scoping

The `POST /agents/{name}/prompt` response includes a `tools` array.
The client can use this to filter which tools are shown to the LLM:

```json
{
  "tools": ["search", "get"]
}
```

Smart clients (Cursor, Claude Desktop) can use this to restrict the
LLM's function-calling capabilities to only the listed tools.

### 10.2 Server-Level Scoping (Future)

A future extension could add an `?agent=code-reviewer` query parameter
to `GET /tools/list` that filters the returned tools:

```
GET /tools/list?agent=code-reviewer
→ only returns search and get tools
```

This enables tool scoping for clients that don't parse the prompt
response's tool list.

---

## 11. Module Structure

### New Files

```
src/
  agents.rs         — Agent trait, AgentInfo, AgentPrompt, AgentRegistry, TomlAgent
  agent_script.rs   — LuaAgentAdapter, load/resolve/scaffold/test
```

### Modified Files

```
src/config.rs       — Add AgentsConfig, InlineAgentConfig, ScriptAgentConfig
src/server.rs       — Add /agents/list, /agents/{name}/prompt routes
src/main.rs         — Add ctx agent list/test/init subcommands
src/lib.rs          — pub mod agents, agent_script
```

---

## 12. Implementation Plan

### Phase 1: Core (MVP)

1. Define `Agent` trait, `AgentPrompt`, `AgentRegistry` in `agents.rs`
2. Add `[agents]` config section to `config.rs`
3. Implement `TomlAgent` for inline TOML agents
4. Add `GET /agents/list` and `POST /agents/{name}/prompt` to server
5. Add `ctx agent list` CLI command
6. Integration tests

### Phase 2: Lua Agents

7. Implement `LuaAgentAdapter` using existing `lua_runtime.rs`
8. Add context bridge (`search`, `get`, `sources`) to agent scripts
9. Add `[agents.script.*]` config support
10. Add `ctx agent test` and `ctx agent init` CLI commands

### Phase 3: MCP Prompts

11. Add `prompts/list` and `prompts/get` MCP protocol handlers
12. Tool scoping via prompt response

### Phase 4: Extensions

13. Rust `Agent` trait in public API (like Connector/Tool)
14. `run_server_with_agents()` for custom harness binaries
15. Dynamic tool list filtering via `?agent=` query parameter

---

## 13. Interaction Patterns

### 13.1 Cursor Workflow

```
1. User opens Cursor chat
2. User types: /incident-responder service=payments-api severity=P1
   (or selects from prompt picker if Cursor supports MCP prompts)
3. Cursor calls: POST /agents/incident-responder/prompt
   body: {"arguments": {"service": "payments-api", "severity": "P1"}}
4. Server runs agent.resolve() → pre-searches KB for runbooks
5. Server returns: system prompt + context messages + tool list
6. Cursor sets system prompt, injects context messages
7. Cursor shows only scoped tools (search, get, create_jira_ticket)
8. User describes the incident
9. LLM uses search tool to find relevant docs
10. LLM uses create_jira_ticket to create tracking ticket
11. All grounded in the knowledge base
```

### 13.2 Claude Desktop Workflow

Same as above, but via MCP prompt capability:

```
1. Claude Desktop discovers agents via prompts/list
2. User selects "incident-responder" from prompt picker
3. Claude calls prompts/get with arguments
4. System prompt and context messages are injected
5. User chats naturally with KB-grounded agent
```

### 13.3 Custom Integration

For a Slack bot, internal tool, or custom UI:

```
1. App calls GET /agents/list to show available agents
2. User picks "code-reviewer"
3. App calls POST /agents/code-reviewer/prompt
4. App gets system prompt + tool list
5. App passes system prompt to their LLM provider (OpenAI, etc.)
6. App proxies tool calls to POST /tools/{name}
7. LLM responds with KB-grounded code review
```

---

## 14. Example: Complete Config

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"

# ── Connectors ──────────────────────────────────

[connectors.git.platform]
url = "https://github.com/acme/platform.git"
branch = "main"
root = "docs/"

[connectors.git.runbooks]
url = "https://github.com/acme/runbooks.git"
branch = "main"

# ── Tools ───────────────────────────────────────

[tools.script.create_jira_ticket]
path = "tools/create-jira-ticket.lua"
url = "https://acme.atlassian.net"
api_token = "${JIRA_API_TOKEN}"

# ── Agents (inline) ────────────────────────────

[agents.code-reviewer]
description = "Reviews code against project conventions"
tools = ["search", "get"]
system_prompt = """
You are a senior code reviewer. Use search to find coding
conventions, then review the user's code against them.
"""

[agents.architect]
description = "Answers architecture questions"
tools = ["search", "get", "sources"]
system_prompt = """
You are a software architect. Search for ADRs and design
docs to ground your recommendations.
"""

# ── Agents (Lua) ───────────────────────────────

[agents.script.incident-responder]
path = "agents/incident-responder.lua"
timeout = 30
search_limit = 5
```

---

## 15. Stability

The public contract is defined by:
- `AGENTS.md` (this document)
- The `context_harness::agents` module
- The `/agents/*` HTTP endpoint schemas

Changes to the `Agent` trait or `AgentPrompt` structure constitute
breaking changes and require a major version bump.

---

## 16. Pre-Commit Checklist

Before committing and pushing changes to this repository, **always** run
the following checks and fix any issues:

```bash
# 1. Format — must produce no diffs
cargo fmt --all -- --check

# 2. Lint — must produce no warnings (warnings are errors in CI)
cargo clippy -- -D warnings

# 3. Test — all tests must pass
cargo test

# 4. Build — release build must succeed
cargo build --release
```

CI enforces all four checks. A commit that fails any of them will block
the release pipeline. Run them locally before pushing to avoid
round-tripping through CI.

