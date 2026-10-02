# PRD-0013: Declarative Tool Bindings and Runtime Extensions

**Status:** Delivered
**Date:** 2026-09-30
**Author:** Context Harness contributors

## Problem Statement

Context Harness can declare an agent's model, instructions, tools and permissions,
but executable agents still use a fixed local tool set plus discovered MCP tools.
Configured Lua tools and Rust extensions available to the existing server do not
form a complete runtime extension path. Adding a domain-specific agent therefore
risks adding domain-specific dispatch and permissions to the runtime itself.

Users need to compose reusable capabilities into named, constrained tools and then
select those tools in agent resources. Extension authors need a supported way to
supply implementations without modifying the runtime loop. The original principle
remains: code implements capabilities; declarative resources compose capabilities.

The local wiki manager exposed this gap. Its detailed PRD/design have been withdrawn
from the working tree at the user's request (retired document number 0012; history
remains in Git). Wiki-specific planning and implementation resume only after this
underlying engineering is complete. This PRD concerns the reusable foundation.

## Target Users

- Agent authors composing tools and models through project/global configuration.
- Extension authors providing Rust, Lua or external MCP implementations.
- Host integrators enforcing narrower filesystem/source authority and approvals.
- Developers installing application resources without changing the runtime loop.

## Goals

1. Declare two independently scoped public tools backed by one implementation,
   and use both from agent resources without new tool-name dispatch in core code.
2. Resolve tools reproducibly with visible provenance, versions and collision errors.
3. Prevent model arguments or resource declarations from expanding host authority.
4. Execute eligible extensions through the existing registry, policy and history path.
5. Demonstrate the same mechanism with two unrelated fixture use cases before
   declaring the foundation ready for a real application such as wiki curation.

## Non-Goals

- Wiki tools, Markdown curation rules, Claude hooks or automatic wiki publication.
- A local inference provider, generic job queue, executable-agent MCP API, or analytics
  platform in this milestone. These remain separate integration work when revisited.
- Creating arbitrary behavior from configuration without an implementation.
- Claiming a permission label sandboxes arbitrary Lua, Rust or external processes.
- Automatically installing/downloading/running extensions during discovery.
- Changing existing stateless MCP prompt contracts or multi-workspace opt-in behavior.

## User Stories

- I bind a generic reader to an enrolled directory and give the named tool to an
  agent; changing an argument cannot read outside that directory.
- I bind retrieval to a source and inspect the effective schema and restrictions
  before the agent runs; a second agent can use a different binding safely.
- I supply an approved extension through a documented interface without editing
  runtime name checks or bypassing approvals/history.
- I change a tool implementation or binding and receive an explicit recovery
  compatibility decision rather than silently resuming with different authority.

## Requirements

| ID | Requirement |
|---|---|
| D1 | Named tool resources identify implementations and configuration independently of agent definitions. |
| D2 | Discovery has deterministic precedence, explicit overrides, collision detection and provenance; explicit config remains isolated. |
| D3 | Public schemas, fixed configuration and model-visible arguments are distinguishable and validated. |
| D4 | Host-granted authority bounds binding and agent authority; unknown or unenforceable restrictions fail closed. |
| D5 | Eligible built-in/Rust, Lua and MCP implementations use the existing ToolRegistry and common invocation lifecycle, with explicit trust requirements. |
| D6 | Users can list, inspect and validate resolved tools without executing them or exposing secrets. |
| D7 | History and recovery identify effective bindings and implementations; incompatible changes cannot silently alter resumed work. |
| D8 | Existing agent, CLI, Lua/Rust extension and MCP contracts remain compatible unless an explicitly documented migration is required. |

## Success Criteria

- Two unrelated fixture applications compose declared tools without adding runtime
  branches for their names; aliases retain implementation validation and policy.
- Tests reject path/source escapes, attempts to override fixed arguments, unknown
  implementations, duplicate names and forged capability claims before effects.
- Discovery/inspection tests start no processes, make no network calls, and never
  emit credential values. Explicit config does not import unrelated resources.
- One approved extension of each supported backend kind exercises approval,
  cancellation, bounded output and durable history; unsupported trust modes produce
  actionable errors rather than being silently enabled.
- Binding/implementation changes are covered by version/recovery tests and legacy
  compatibility regressions. Existing external-session resume restrictions remain.

## Dependencies and Risks

Depends on PR #33's runtime/registry foundation. Lua host APIs and external process
access require enforceable authority contracts, not self-asserted metadata. Aliases
can hide privilege or collide with existing names; inspection and validation need
to expose effective behavior. Generic argument mapping can become a programming
language; the initial scope should be fixed bindings, not arbitrary expressions.

## Resolved Questions

SPEC-0023 resolves the initial resource schema/location, capability descriptor and
binding contract, eligible Lua trust mode, MCP trust and argument-schema validation,
extension identity/versioning, and migration rules. DESIGN-0013 retains the
implementation sequence and acceptance gates.

## Related Documents

- [DESIGN-0013](../design/0013-declarative-tool-bindings.md): proposed architecture,
  gap audit, implementation order and handoff checklist.
- [ADR-0025](../adr/0025-declarative-tool-bindings.md): proposed architectural boundary.
- [DESIGN-0010](../design/0010-local-agent-runtime.md): original architectural intent.
- [DESIGN-0011](../design/0011-local-agent-runtime-execution-plan.md): completed initial
  slices, explicitly not completion of runtime extension composition.
- [SPEC-0023](../spec/0023-declarative-tool-bindings.md): authoritative resource,
  authority, runtime, backend, inspection, and recovery contract.
- Future runbook: create with verified setup, inspection and migration behavior.
