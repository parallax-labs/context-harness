# SPEC-0022: Resource Prompt Projection

**Status:** Authoritative
**Date:** 2026-09-30
**Related:** [DESIGN-0010](../design/0010-local-agent-runtime.md), [resources](0015-agent-resources.md), [MCP agents](0011-mcp-agents.md)

## Contract

Single-workspace `ctx serve mcp` SHALL discover standalone agent resources using
the same resolved-config directory selection, validation and override rules as
`ctx agent list`. Explicit `--config`, `CTX_CONFIG`, and pinned configurations
SHALL retain config-adjacent isolation. Default discovery SHALL combine global
and cwd `.ctx/agents` resources. Invalid selected resources SHALL fail startup.

Resources SHALL register through the existing `Agent` trait as static TOML prompt
agents. MCP `prompts/list` SHALL expose their name and description with no prompt
arguments. `prompts/get` SHALL return `prompt.system` as user-role context, matching
the existing inline TOML conversion (MCP has no system message role). Repeated
resolution SHALL remain stateless. Static TOML prompts retain existing behavior
of ignoring supplied arguments. REST `/agents/list` and `/agents/{name}/prompt`
SHALL expose the same registered agents, including their declared tool hints.

Projection SHALL NOT instantiate a model provider, resolve credentials, execute
an agent, create run history, start configured external MCP servers, or register
runtime tools on the serving MCP endpoint. Model aliases and resource declarations
are validated structurally; runtime tool availability is not a prompt prerequisite.
Runtime permission declarations and execution budgets SHALL NOT be interpreted as
permissions or enforcement for external clients. Clients own their execution and
permission policy; declared tools may be unavailable in their environment.

A resource name conflicting with configured inline/Lua agents, registry Lua
agents, or caller-supplied Rust agents SHALL fail startup. All resource collision
checks SHALL finish before any resource is registered. Legacy resolution and
precedence between existing legacy sources remain unchanged.

## Library and multi-workspace compatibility

Existing `run_server` and `run_server_with_extensions` callers SHALL retain their
behavior without implicit filesystem discovery. Library callers MAY opt in using
`run_server_with_resources`, supplying explicit resource directories and extension
registries. `register_resource_prompts` provides the same registration checks for
embedded integrations.

Multi-workspace serving SHALL continue to expose only its built-in tools and no
workspace agents, as specified by SPEC-0014 R54. It SHALL NOT import resources from
the process cwd or any registered workspace.

## Validation and boundaries

Protocol tests exercise real MCP initialization/list/get over an in-memory byte
transport, resource/legacy/extension prompts, collision rejection and the absence
of runtime database creation or tool exposure. CLI tests exercise config-isolated
server discovery. Existing Lua/Rust and multi-workspace regressions remain gates.
A deterministic implementer demo reads and patches a temporary configuration file,
runs real local tests with explicit approval, delegates a read-only review, and
asserts durable tool results, approvals and child lineage. Fake model responses
make this reproducible without provider credentials; it is not a live model test.

This completes the nine-slice execution plan, not every optional extension in the
original design. Streaming, remote MCP transports/OAuth, concurrent delegation,
a local queue, and recovery of delegated or external-session runs remain outside
this implementation. The platform and process-sandbox limits in SPEC-0018 through
SPEC-0021 still apply.
