# ADR-0024: Optional Local Agent Execution

**Status:** Accepted  
**Date:** 2026-09-25  
**Related:** [DESIGN-0010](../design/0010-local-agent-runtime.md), [execution plan](../design/0011-local-agent-runtime-execution-plan.md), [ADR-0014](0014-stateless-agent-architecture.md)

## Context

ADR-0014 describes agents as stateless prompt generators whose clients own model
invocation and conversations. Local execution needs durable run state without
changing the existing MCP prompt contract.

## Decision

Support two modes: stateless prompt projection for external clients and optional
runtime-owned execution in `ctx`. Existing prompt resolvers remain usable. The
local runtime will own model/tool iteration, permissions and run history, with
SQLite as canonical persistence in the existing application database.

This supersedes ADR-0014 only as the complete definition of an agent. Its
statelessness decision still governs MCP prompt resolution. Persisted local runs
do not create implicit MCP conversation sessions.

Use existing workspace, context and tool boundaries. Do not require a broker,
external database, object store or separate service. Implement incrementally;
this decision does not imply that runtime commands have already shipped.

## Consequences

- Runtime history can be tested without a provider or agent loop.
- Workspace binding and authorization are enforced by runtime code.
- Resume needs durable checkpoints and explicit handling of uncertain tool side
  effects; an event log alone does not guarantee replay safety.
- Existing integrations retain the external-client execution model.
- Runtime lifecycle and provider concerns add application code, with a future
  internal crate split available once those boundaries stabilize.

## Alternatives Considered

Keeping all execution in external clients cannot provide locally owned history
and resume. A separate agent application would duplicate context, workspace and
tool integration. Distributed execution infrastructure is unnecessary for the
single-user local workflow.
