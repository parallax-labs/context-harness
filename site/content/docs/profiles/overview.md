+++
title = "Profiles Overview"
description = "Make Cursor, Claude, and custom clients start with the right role and context without moving execution into Context Harness."
weight = 1
+++

A Context Harness profile is a reusable starting recipe for an AI client you
already use. It packages a role, suggested tools, arguments, and optional
project context as an MCP prompt.

Instead of beginning every conversation with a long setup message, select or
resolve a named profile such as `code-reviewer`, `architect`, or
`incident-responder`.

```
You choose: code-reviewer
          │
          ▼
Context Harness resolves the profile
  • system instructions
  • suggested Context Harness tools
  • optional project context
          │
          ▼
Cursor, Claude, or your application owns the conversation
```

### The use case

Profiles are useful when the AI experience already exists and you want every
conversation to begin with the same project-aware operating instructions.

For example, a `code-reviewer` profile can tell the client to search for project
conventions before reviewing, retrieve full documents when excerpts are
ambiguous, cite evidence, and use only the relevant Context Harness tools. A Lua
profile can also fetch current runbooks or architecture context when the profile
is selected.

The result is a repeatable role without asking Context Harness to host another
model session.

### What a profile controls

| Context Harness profile supplies | External client still owns |
|---|---|
| System instructions | Model selection and credentials |
| Suggested tool names | Tool availability and enforcement |
| Argument schema | Conversation UI and user interaction |
| Optional pre-resolved messages or context | Conversation history |
| A stable name shared by a team | Model calls and execution loop |

A profile does not execute tools, create a durable run, enforce agent
permissions, or remember earlier conversations. Those responsibilities remain
with the client.

### How clients receive profiles

Profiles are exposed through the MCP Prompts protocol. MCP clients that support
prompts can list and resolve them through their own interface. Client support and
presentation vary, so selecting a profile may appear as a prompt picker, command,
or integration-specific action.

Context Harness also exposes compatibility REST endpoints for custom clients:

```sh
curl -s http://127.0.0.1:7331/agents/list

curl -s -X POST \
  http://127.0.0.1:7331/agents/code-reviewer/prompt \
  -H 'Content-Type: application/json' \
  -d '{}'
```

The `/agents/*` route and `[agents.*]` configuration names are historical. They
now represent what this documentation calls profiles.

Connecting Context Harness as an MCP server makes its capabilities discoverable,
but it does not silently apply a profile to every chat. The user or client
selects the profile for the conversation that needs it.

### Reuse an executable agent as a profile

You do not have to maintain two role definitions. When single-workspace
`ctx serve mcp` discovers `.ctx/agents/*.toml`, it projects each standalone
agent's name, description, system prompt, and tool hints as a stateless MCP
profile.

```
.ctx/agents/code-reviewer.toml
              │
       ┌──────┴──────┐
       ▼             ▼
ctx agent run    MCP profile
ctx owns work    client owns chat
```

The projection intentionally does not initialize the configured model, resolve
credentials, execute tools, create run history, or apply the agent's runtime
permissions and budgets. The external client owns those concerns. This gives you
one reusable role with two explicit execution choices.

### Static and dynamic profiles

Use an inline TOML profile when the role and instructions are stable:

```toml
[agents.inline.code-reviewer]
description = "Reviews changes against project conventions"
tools = ["search", "get"]
system_prompt = """
Search for relevant project conventions before reviewing. Cite the evidence
behind correctness findings and separate them from optional improvements.
"""
```

Use a Lua profile when startup context depends on arguments or current indexed
knowledge. For example, an incident profile can accept `service` and `severity`,
search for relevant runbooks, and place those results into the initial prompt.

Custom embedding applications can also implement the historically named Rust
`Agent` trait to resolve an `AgentPrompt`. Despite that API name, resolving the
prompt still does not start a model loop.

### Profiles versus agents

Choose a **profile** when you want to improve an existing AI client's
conversation. Choose an **agent** when you want Context Harness to execute,
govern, persist, and potentially recover the task itself.

Profiles keep adoption lightweight: no additional model configuration in
Context Harness, no second conversation history, and no duplicate execution
loop. Agents provide stronger operational guarantees when the task must outlive
the client session or needs runtime-enforced authority.

See [Agents Overview](@/docs/agents/overview.md) for the executable path.

### Start here

1. Open [Define Profiles](@/docs/profiles/define-profiles.md) for inline TOML,
   Lua, Rust, CLI, MCP, and REST examples.
2. Start the server with `ctx serve mcp`.
3. Confirm the profile appears with `ctx agent list` or `GET /agents/list`.
4. Select it from an MCP prompt-capable client or resolve it from your own
   integration.

If you instead need `ctx agent run`, durable history, approvals, inspection, or
resume, begin with [Build Local Agents](@/docs/agents/build-local-agents.md).
