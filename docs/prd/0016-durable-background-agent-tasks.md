# PRD-0016: Durable Background Agent Tasks

**Status:** Draft
**Date:** 2026-10-02
**Author:** Context Harness contributors

## Problem Statement

`ctx agent run` owns a bounded run in the invoking process. This is appropriate for
interactive work, but a local hook or embedding application cannot durably accept
work, return quickly, and let an independent worker execute it later. Detaching a
process does not provide duplicate suppression, transactional claiming, queue bounds,
observable lifecycle, or conservative restart behavior.

Applications need an optional, domain-neutral asynchronous task layer that drives the
existing runtime rather than creating a second agent loop. Context Harness must keep
accepted work distinct from a concrete run attempt and must not become a general
workflow engine.

## Target Users

- Local applications and hooks that submit agent work without waiting for completion.
- Operators running independently managed Context Harness workers.
- Rust hosts that register tools and correlate application jobs with agent history.
- Agent authors whose resources should run identically in direct and background modes.

## Goals

1. Durably accept a named agent task and return its stable identity before execution.
2. Suppress exact duplicate submissions and reject request-key reuse with different
   immutable input.
3. Claim tasks transactionally across workers with bounded concurrency and attempts.
4. Preserve existing policy, approvals, history, checkpoint, cancellation, and
   conservative recovery behavior for every resulting run.
5. Provide a supported public host lifecycle for registering implementations,
   submitting tasks, operating workers, and correlating task/run identities.
6. Keep synchronous `ctx agent run` unchanged and primary for direct execution.

## Non-Goals

- Distributed scheduling, cron, dependencies, DAGs, priorities, or a workflow DSL.
- Application-specific evidence, publication, hook parsing, or job states.
- Remote unauthenticated submission or arbitrary agent execution over MCP/HTTP.
- Cross-run memory, richer human interaction, or analytics.
- Automatically starting a worker during server, sync, or inspection commands.
- Defining local generation; tasks work with any configured provider.

## User Stories

- I submit a task from a short-lived hook and receive a durable ID before exiting.
- Repeating the same request key and input returns the accepted task; changing the
  input under that key produces a visible conflict.
- I start a worker later and it executes the same declared agent/runtime path as
  `ctx agent run`.
- I run two workers and know only one can own a task.
- After a crash, I can distinguish safe requeue, resumable run, terminal run, and
  uncertain side effect without silent duplicate execution.
- As an embedding host, I register scoped tools and authority through public APIs and
  correlate my application job, Context Harness task, and agent run.

## Requirements

| ID | Requirement |
|---|---|
| B1 | Submission durably records agent, immutable input identity, workspace, accepted resource identity, and caller request key before returning. |
| B2 | Submission performs no model or tool execution; static resolution remains side-effect free. |
| B3 | Exact duplicate request-key/input submissions return one task; key reuse with different input conflicts. |
| B4 | Workers use transactional claims and bounded leases, concurrency, queue depth, and attempt counts. |
| B5 | A task references concrete agent run attempts without weakening run ownership, terminal immutability, checkpoint identity, or unfinished-effect recovery rules. |
| B6 | Cancellation distinguishes queued cancellation, requested running cancellation, and confirmed terminal outcome. |
| B7 | Task inspection exposes scheduling/ownership state and projects the linked run's authoritative typed execution outcome without defining a competing run-lifecycle taxonomy. |
| B8 | Public host APIs support trusted provider/tool registration, host authority, submission, worker operation, cancellation, and task/run correlation. |
| B9 | Existing synchronous execution, history, resume, profiles, bindings, and multi-workspace isolation remain compatible. |

## Success Criteria

- A submitter exits after acceptance; a worker started later completes the task.
- Contention tests prove two workers cannot own the same claim.
- Duplicate, key/input conflict, queue-full, cancellation, lease-expiry, resource-drift,
  and crash-injection tests produce deterministic outcomes.
- Recovery tests never create a second run or replay an uncertain tool invocation
  automatically.
- Two unrelated fixture hosts use the public API with scoped compiled tools and no
  private-module access or runtime public-name dispatch.
- No worker starts during list/show/validate, `ctx serve`, or ordinary sync.
- Existing direct agent execution and recovery tests remain green.

## Dependencies and Risks

This work depends on the existing `AgentRunStore`, runtime ownership locks, checkpoint
verification, declarative binding identities, and the run-lifecycle contract proposed
by PRD-0014. A task is accepted intent; a run is an execution attempt. Task states
describe scheduling and ownership; run states and typed outcomes describe execution.
Conflating them risks duplicate effects, competing lifecycle taxonomies, and ambiguous
recovery. Leases can expire while the underlying process is still alive, so expiry
alone cannot authorize a second run. SQLite contention and clock assumptions require
explicit bounds.

## Resolved Decisions

Resolved by [ADR-0029](../adr/0029-durable-agent-task-ownership.md) and
[SPEC-0026](../spec/0026-durable-agent-tasks.md): the main crate owns a separate task
store; submission pins a versioned non-secret execution identity and fails visibly on
drift; version one permits one immutable run link; claims use bounded database-clock
leases and attempts; Phase 4A is library-first and CLI surfaces remain Phase 4D.

## Related Documents

- [DESIGN-0016](../design/0016-durable-background-agent-tasks.md): task/run split,
  claim/recovery design, host builder, and implementation slices.
- [PRD-0014](0014-durable-agent-runs.md) and
  [DESIGN-0014](../design/0014-structured-agent-run-state.md): typed run outcomes,
  budgets, working state, context projection, and recovery semantics consumed by the
  task layer but not duplicated by it.
- [DESIGN-0010](../design/0010-local-agent-runtime.md): direct runtime and optional
  SQLite task-queue direction.
- [SPEC-0017](../spec/0017-local-agent-execution.md): current direct execution.
- [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md): recovery rules.
- [SPEC-0023](../spec/0023-declarative-tool-bindings.md): binding authority/identity.
- [PRD-0015](0015-local-generation-providers.md): independent local provider work.
- [ADR-0029](../adr/0029-durable-agent-task-ownership.md): task/run authority,
  exactly-one-run ownership, claims and cancellation rationale.
- [SPEC-0026](../spec/0026-durable-agent-tasks.md): authoritative task identity,
  persistence, submission, worker and reconciliation behavior.
