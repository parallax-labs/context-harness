# ADR-0014: Stateless Agent Architecture

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness agents are reusable AI personas that combine a system prompt,
scoped tools, and optional dynamic context injection. They need to work
across multiple MCP clients (Cursor, Claude Desktop, custom integrations)
that each manage conversations differently.

The agent system must decide whether the server manages conversation state
(sessions, chat history, memory) or whether agents are stateless prompt
generators that delegate state management to the client.

## Decision

Agents are **stateless prompt generators**. Each `resolve()` call is
independent — the server maintains no sessions, no chat history, and no
per-conversation state.

When a client activates an agent:

1. Client calls `POST /agents/{name}/prompt` (or MCP `prompts/get`) with
   optional arguments.
2. Server executes `agent.resolve(args, ctx)`, which may perform dynamic
   context injection (searching the KB, fetching documents).
3. Server returns an `AgentPrompt` containing:
   - `system` — the system prompt text
   - `tools` — which tools should be visible
   - `messages` — optional pre-seeded conversation messages
4. Client uses the system prompt and tools for its conversation. The server
   is not involved in subsequent turns.

Agents are defined in two modes (see [ADR-0007](0007-trait-based-extension-system.md)):

- **Inline TOML** — static system prompt and tool list in `ctx.toml`
- **Lua script** — dynamic prompt generation with KB access at resolve time

The `Agent` trait mirrors the `Connector` and `Tool` traits:

```rust
trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn tools(&self) -> Vec<String>;
    fn arguments(&self) -> Vec<AgentArgument>;
    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<AgentPrompt>;
}
```

MCP mapping: agents are exposed as **MCP prompts** via `prompts/list` and
`prompts/get`, which is the standard MCP mechanism for reusable prompt
templates.

## Alternatives Considered

**Server-side sessions.** The server maintains conversation state, including
chat history, retrieved context, and tool call results. This enables
server-side features like conversation summarization, automatic
re-retrieval, and cross-session memory. However:
- Couples the server to specific client behavior (turn-taking, message
  format).
- Requires session storage, lifecycle management, and garbage collection.
- Different MCP clients manage conversations differently — imposing a
  server-side model creates friction.
- Dramatically increases server complexity for a feature that clients
  already handle well.

**Fine-tuned models per agent.** Train or fine-tune a model for each persona.
Produces the most authentic agent behavior but is extremely expensive,
not portable across model providers, and impractical for personal-scale
tooling where agents are frequently created and modified.

**Chat completion proxy.** The server acts as a proxy to an LLM provider,
injecting the agent prompt and forwarding messages. This would make Context
Harness a full chat backend. However, it duplicates functionality that
Cursor and Claude Desktop already provide, requires API key management for
inference (not just embedding), and blurs the system's identity as a
knowledge base tool.

## Consequences

- Agents work identically across all MCP clients — Cursor, Claude Desktop,
  custom Slack bots, internal tools. The client handles conversation state.
- No session storage or cleanup logic on the server. The server remains a
  simple request-response service.
- Lua agents can perform knowledge base searches at resolve time, injecting
  relevant context into the system prompt before the conversation starts
  (RAG-priming). This provides dynamic behavior without server-side state.
- Tool scoping (exposing only agent-relevant tools) reduces hallucination
  and noise in the LLM's function-calling decisions.
- The tradeoff is that agents cannot "remember" across conversations or
  learn from past interactions. This is acceptable for the target use case
  (knowledge-grounded task assistance) and avoids the complexity of
  conversation persistence.
- Adding server-side memory later (e.g., `ctx agent history`) is possible
  without changing the stateless prompt generation model — memory would be
  injected as additional context during `resolve()`.
