# SPEC-0024: Profiles and Agent Compatibility

**Status:** Accepted  
**Date:** 2026-10-01

## Purpose

Context Harness has two different execution models:

- a **profile** resolves a reusable prompt for an external MCP or REST client;
- an **agent** runs a model-and-tool loop inside Context Harness and persists a
  durable run.

The original prompt subsystem used the name “agent.” This specification makes
profile the canonical name without breaking existing configurations or clients.

## Canonical profile surface

New integrations SHALL use:

- `[profiles.inline.<name>]` and `[profiles.script.<name>]` in configuration;
- a global `profile` table and `profile.resolve(args, config, context)` in Lua;
- `Profile`, `ProfileArgument`, `ProfileInfo`, `ProfilePrompt`,
  `ProfileRegistry`, and `TomlProfile` in Rust;
- `ctx profile list|show|validate|test|init` in the CLI;
- `GET /profiles/list` and `POST /profiles/{name}/prompt` over REST;
- `profiles/<name>/profile.lua` for registry discovery.

MCP continues to expose profiles through the standard `prompts/list` and
`prompts/get` methods.

## Executable agent surface

The `ctx agent` command and `.ctx/agents/*.toml` resources are reserved for
executable local agents. An agent resource MAY be projected as a stateless MCP
profile, but profile resolution SHALL NOT initialize its model, resolve
credentials, execute tools, create run history, or enforce its runtime policy.

## Compatibility

The following historical spellings SHALL remain accepted during the deprecation
period:

- `[agents.inline.*]` and `[agents.script.*]`;
- Lua `agent` and `agent.resolve`;
- the Rust `agents` and `agent_script` modules and their former type/function
  names;
- `ctx agent test` and `ctx agent init`;
- `/agents/list` with its historical `{ "agents": [...] }` response and
  `/agents/{name}/prompt`;
- registry `[agents.*]`, `agents/<name>/agent.lua`, and `agents/<name>` lookup.

Canonical output, generated examples, scaffolding, documentation, and server
startup messages SHALL use profile terminology. Compatibility aliases MUST
delegate to the same implementation rather than maintaining a second prompt
subsystem.

## Conversation history

Profiles do not own a conversation and SHALL NOT persist conversation history.
The external client owns the session created from a profile. Executable agents
persist each run for inspection and safe resume, but a new run SHALL NOT silently
inject transcripts from previous runs into model context.
