# PRD-0003: Extensibility Platform

**Status:** Delivered
**Date:** 2026-02-22 (conceived) / 2026-02-27 (documented)
**Author:** pjones

## Problem Statement

Context Harness ships with built-in connectors for filesystem, Git, and S3,
but teams need to ingest from dozens of other systems: Jira, Confluence,
Slack, Notion, GitHub Issues, Linear, RSS feeds, internal APIs, and
proprietary SaaS tools. Building every connector in compiled Rust is
unsustainable -- each new source requires a code change, recompilation,
and a new release.

Beyond connectors, AI tools need custom capabilities: summarizing documents,
linking tickets, querying external APIs, running domain-specific analysis.
And different teams need different agent personas (code reviewer, incident
responder, onboarding guide) with scoped tools and tailored system prompts.

The system needs a layered extensibility model: scripting for rapid
development and community contribution, compiled traits for performance-
critical or type-safe extensions, and a distribution mechanism for sharing
extensions across teams.

## Target Users

1. **End users** who want to connect Context Harness to their team's
   systems (Jira, Confluence, Slack) without writing Rust.
2. **Extension authors** who want to publish connectors, tools, and agents
   for the community.
3. **Power users / library consumers** who build custom Context Harness
   binaries with compiled Rust connectors and tools.
4. **AI agents** (Cursor, Claude) that discover and call tools during
   conversations, and adopt different personas via agent prompts.

## Goals

1. Provide a Lua scripting runtime for connectors, tools, and agents
   that requires zero Rust compilation.
2. Provide Rust traits (`Connector`, `Tool`, `Agent`) for compiled
   extensions with full type safety.
3. Provide an agent system with named personas, scoped tools, and dynamic
   context injection via system prompts.
4. Provide a Git-backed extension registry for sharing connectors, tools,
   and agents across teams and the community.
5. Support a resolution precedence model: project-local > personal >
   company > community.
6. Ship CLI commands for developing, testing, and managing extensions.

## Non-Goals

- A centralized extension marketplace with accounts and billing.
- Dynamic library / plugin loading (`.so` / `.dylib`).
- Connectors that require their own background processes or daemons.

## User Stories

**US-1: Install community connectors.**
A user runs `ctx registry init` and gets a curated set of Lua connectors
(Jira, Confluence, Slack, GitHub Issues, etc.). They run
`ctx registry add connectors/jira`, fill in their API credentials in
`ctx.toml`, and `ctx sync script:jira` indexes their Jira project.

**US-2: Write a custom connector.**
A user runs `ctx connector init my-api` to scaffold a Lua connector. They
implement `connector.scan(config)` using the built-in `http` and `json`
host APIs. They test with `ctx connector test my-api` and add it to their
config. No Rust, no compilation.

**US-3: Create an MCP tool.**
A user writes a Lua tool that calls an external API (e.g., create a Jira
ticket). The tool receives search context via `context.search` and
`context.get`. It is auto-discovered by the MCP server and available to
Cursor immediately.

**US-4: Define an agent persona.**
A user creates a `code-reviewer` agent with a system prompt, scoped tools
(search, get), and dynamic context injection. When Cursor requests
`prompts/get code-reviewer`, it receives a tailored system prompt with
relevant context pre-loaded.

**US-5: Build a custom binary.**
A power user creates a Rust binary that depends on `context-harness`,
registers compiled connectors and tools via `ConnectorRegistry` and
`ToolRegistry`, and runs the server with `run_server_with_extensions()`.

**US-6: Override a community extension.**
A user runs `ctx registry override connectors/jira` to copy the community
Jira connector to their personal path. They customize it for their team's
Jira setup. Their version takes precedence over the community version.

## Requirements

### Lua Runtime

1. The system SHALL embed Lua 5.4 via the `mlua` crate.
2. Lua scripts SHALL have access to host APIs: `http`, `json`, `env`,
   `log`, `fs`, `base64`, `crypto`, `sleep`.
3. Lua connectors SHALL implement `connector.scan(config)` returning a
   list of `SourceItem` tables.
4. Lua tools SHALL implement `tool.execute(params, context)` where
   `context` provides `search`, `get`, and `sources`.
5. Lua agents SHALL implement `agent.prompt(context)` returning a system
   prompt string.

### Rust Traits

6. The system SHALL define `Connector`, `Tool`, and `Agent` traits in Rust.
7. The system SHALL provide `ConnectorRegistry`, `ToolRegistry`, and
   `AgentRegistry` for registering compiled extensions.
8. The system SHALL provide `run_server_with_extensions()` and
   `run_sync_with_extensions()` for custom binaries.

### Agent System

9. Agents SHALL be configurable via inline TOML (`[agents.inline.*]`)
   or Lua scripts (`[agents.script.*]`).
10. Agents SHALL be exposed via `GET /agents/list` and
    `POST /agents/{name}/prompt` HTTP endpoints.
11. Agents SHALL be exposed via MCP `prompts/list` and `prompts/get`.
12. Agents SHALL support scoped tool lists and dynamic context injection.

### Registry

13. The system SHALL support Git-backed extension registries configured
    in `[registries.*]` sections of `ctx.toml`.
14. Each registry SHALL contain a `registry.toml` manifest describing
    available extensions with metadata, required config, and tags.
15. The system SHALL resolve extensions in precedence order: explicit
    `ctx.toml` > `.ctx/` project-local > personal > company > community.
16. The system SHALL provide CLI commands: `registry list`, `install`,
    `update`, `search`, `info`, `add`, `override`, `init`.
17. Tools and agents from registries SHALL be auto-discovered at server
    startup without explicit config entries.
18. Connectors from registries SHALL require explicit activation via
    `ctx registry add` (because they need credentials).

### CLI

19. The system SHALL provide: `connector init`, `connector test`,
    `tool init`, `tool test`, `tool list`, `agent init`, `agent test`,
    `agent list`.

## Success Criteria

- A new Lua connector can be written, tested, and used in <15 minutes.
- The community registry has 6+ connectors, 3+ tools, and 2+ agents.
- `ctx registry install` completes in <5 seconds.
- AI tools (Cursor) can discover and call custom tools and request agent
  prompts via MCP.
- A custom Rust binary can register extensions and run the server with
  <20 lines of glue code.

## Dependencies and Risks

- **Lua ecosystem:** Lua has no npm/pip equivalent. Compensated by
  providing rich host APIs (HTTP, JSON, crypto, etc.) built into the
  runtime.
- **Registry maintenance:** Community registries need ongoing curation
  and CI to prevent broken scripts from being published.
- **Security:** Lua scripts run in a sandboxed VM with only explicitly
  exposed host APIs. No arbitrary filesystem or network access beyond
  what the host provides.

## Related Documents

- **ADRs:** [0007](../adr/0007-trait-based-extension-system.md),
  [0008](../adr/0008-lua-for-runtime-extensibility.md),
  [0013](../adr/0013-git-backed-extension-registries.md),
  [0014](../adr/0014-stateless-agent-architecture.md)
- **Specs:** [REGISTRY.md](../REGISTRY.md),
  [AGENTS.md](../AGENTS.md),
  [RUST_TRAITS.md](../RUST_TRAITS.md),
  [LUA_TOOLS.md](../LUA_TOOLS.md),
  [LUA_CONNECTORS.md](../LUA_CONNECTORS.md)
- **PRDs:** [PRD-0001](0001-core-context-engine.md) (core engine these
  extensions plug into)
