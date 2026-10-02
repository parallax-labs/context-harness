# DESIGN-0016: Durable Background Agent Tasks

**Status:** Draft
**Date:** 2026-10-02
**Author:** Context Harness contributors
**Related:** [PRD-0016](../prd/0016-durable-background-agent-tasks.md), [PRD-0014](../prd/0014-durable-agent-runs.md), [DESIGN-0014](0014-structured-agent-run-state.md), [DESIGN-0010](0010-local-agent-runtime.md), [SPEC-0017](../spec/0017-local-agent-execution.md), [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md), [SPEC-0023](../spec/0023-declarative-tool-bindings.md)

## Context

`AgentRuntime::run` validates input, creates an `agent_runs` row in `running` state,
acquires a per-run ownership lock, and immediately drives model/tool execution. The
run store is durable execution history, not a queue. Its states and resume checks
assume execution has begun.

Hooks and embedding applications need quick durable acceptance and independent work.
This design adds an optional task layer around the same runtime. It excludes any
application's domain events and publication lifecycle.

PRD/DESIGN-0014 separately propose typed run outcomes, cumulative budgets, structured
working state and context projection. This design consumes the resulting run-lifecycle
and recovery contract; it does not redefine those execution semantics. The task layer
can be designed while 0014 remains Draft, but implementation of outcome projection
and recovery reconciliation waits for the shared lifecycle behavior to become an
authoritative spec. Local generation and targeted refresh do not share that dependency.

## Proposal

### Task/run separation

Add an `agent_tasks` store. A task represents accepted intent; a run represents one
concrete execution attempt.

```text
submitter -> AgentTaskStore -> queued task
                                  |
worker -> transactional claim ----+
   -> resolve accepted identities
   -> create ordinary AgentRuntime run
   -> project terminal run outcome back to task
```

Conceptual fields include task ID, workspace ID, request key, input digest, agent and
accepted identities, immutable input, status, claim owner/lease, attempt count, run
ID, timestamps, and sanitized error class. Secret values are never stored.

Request-key uniqueness is workspace-scoped. The digest covers agent, input, and a
bounded opaque payload identity supplied by the trusted host. The framework does not
interpret application events. Same key/digest returns the task; same key/different
digest conflicts.

Task state is intentionally limited to scheduling and ownership facts such as queued,
claimed, cancellation requested, and terminal projection recorded. `blocked`,
`needs_user_input`, `limit_exceeded`, and other execution meanings belong to the
linked run contract from 0014. Task inspection references or projects that outcome;
workers do not independently infer it.

### Claim and execution

Workers claim with a conditional SQLite transaction and bounded lease. Before
creating a run, a worker resolves current agent/model/binding identities. The
recommended default is fail-visible on incompatible drift; do not silently execute
with broader or different authority.

The worker records the task/run relationship before model work proceeds. Run events,
tools, checkpoints, and artifacts stay in existing stores. The task stores lifecycle
and the terminal run reference rather than copying model/tool history.

Heartbeat maintains the task claim independently of model turns. Multiple workers may
poll, but only one owns a task. Queue depth, concurrency, lease, polling, attempts,
and graceful shutdown are explicit host settings.

### Reconciliation

Lease expiry does not authorize a new run by itself:

- no run exists: safely requeue within attempt limits;
- referenced run is terminal: project its outcome;
- referenced run is active, suspended, or recoverable: use the authoritative run
  lifecycle plus existing ownership and checkpoint checks;
- unfinished or uncertain effects: mark recovery-required and do not replay;
- resource identity drift: fail or await explicit resubmission per the future spec.

Queued cancellation is immediate. Running cancellation signals the runtime and is not
reported terminal until the run records cancellation or another terminal result.

### Public host lifecycle

Provide an `AgentHostBuilder` or equivalent that assembles resolved config/workspace,
stores, model providers, tool implementation catalog, runtime tool registry, host
authority, resources, and cancellation/progress hooks. It exposes direct execution,
task submission, bounded worker operation, and stable task/run stores.

The builder grants no default write/process/network authority, starts nothing during
inspection, and rejects registry collisions. Two fixture hosts should prove the API
without private modules or tool-name branches.

### CLI shape

Illustrative only:

```sh
ctx agent enqueue <agent> <input> --request-key <key> --json
ctx agent worker --max-concurrency 1
ctx agent jobs --json
ctx agent job inspect <task-id> --json
ctx agent job cancel <task-id> --json
```

The worker is foreground-first and service-manager-neutral. Launchd/systemd examples
belong in the eventual runbook. `ctx serve`, sync, and inspection never start it.

### Multi-workspace boundary

A task is bound to one canonical workspace at submission. A worker enrolls explicit
workspaces and never derives a target from untrusted task input. Cross-workspace list
or claim behavior remains opt-in and must preserve existing router isolation.

## Alternatives Considered

- **Application-owned queues only:** remains supported, but every host would reinvent
  identity, claims, run correlation, and recovery projection.
- **Add `queued` to `agent_runs`:** conflates intent and execution and weakens current
  ownership/checkpoint invariants.
- **Detach `ctx agent run`:** lacks durable acceptance, deduplication, claims, bounds,
  and safe restart reconciliation.
- **External broker:** unnecessary operational burden for a local-first tool.
- **Executable MCP first:** expands remote authentication and agent-selection scope
  before local lifecycle semantics are stable.

## Implementation Plan

1. Specify task schema, identity, scheduling states, and migration separately from
   the typed run lifecycle defined by the 0014 spec work.
2. Implement transactional submission, deduplication, bounds, inspection, and queued
   cancellation without executing providers/tools.
3. Implement claims, leases, heartbeat, concurrency, and graceful shutdown.
4. Link tasks to ordinary runs and implement conservative reconciliation with crash
   injection around claim, run creation, checkpoints, and terminal projection.
5. Publish the host builder and two unrelated fixture integrations.
6. Add approved CLI surfaces and a service-manager-neutral runbook.
7. Write the ADR/spec chain and reconcile it with the 0014 lifecycle/recovery specs
   before implementation behavior becomes authoritative.

## Acceptance Criteria

- Acceptance commits before returning and survives submitter exit.
- Duplicate and key/input conflict behavior is transactional under contention.
- Two workers never hold one claim concurrently.
- Resource drift cannot silently broaden execution authority.
- Crash recovery never starts a second run over an uncertain first attempt.
- Cancellation states are accurate rather than optimistic.
- External hosts register constrained tools and correlate task/run IDs publicly.
- Direct run/resume/history behavior remains unchanged.

## Open Questions

1. Core, CLI, or optional module placement?
2. Exact accepted identity and drift policy?
3. One run per task or explicitly safe multiple attempts?
4. Lease/heartbeat/clock/shutdown defaults?
5. Initial task state names and operator-required recovery representation?
6. Minimum CLI versus library-only surface?

The answer to question 5 must reuse 0014's typed run outcome rather than introduce a
second execution-status vocabulary.
