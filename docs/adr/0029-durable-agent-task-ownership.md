# ADR-0029: Durable Agent Tasks Own Scheduling, Not Execution Outcomes

**Status:** Accepted
**Date:** 2026-10-05

## Context

Context Harness can execute and recover one durable agent run, but it cannot accept
work from a short-lived submitter and let an independently managed worker execute it
later. Reusing `agent_runs` as a queue would conflate accepted intent with execution,
and treating an expired worker lease as permission to start over could duplicate an
uncertain side effect.

The task layer also needs idempotent submission, bounded queue ownership and
cancellation without defining another vocabulary for blocked, needs-input, failed,
limited or cancelled execution. Those meanings already belong to the authoritative
run lifecycle in SPEC-0025.

## Decision

Context Harness SHALL persist background work in a separate, workspace-scoped
`agent_tasks` store. A task owns submission, queueing, worker claim and cancellation
facts. A linked `agent_run` remains the sole authority for execution lifecycle,
outcome, usage, checkpoints, effects and recovery disposition.

Submission SHALL pin a non-secret accepted execution identity resolved through the
trusted host assembly path. It SHALL be idempotent by workspace and request key:
repeating the same key and immutable request digest returns the existing task, while
reusing the key for different immutable input conflicts. Submission performs no
provider construction, credential resolution, database-independent tool execution,
model call or process startup.

The first version SHALL allow exactly one run identity per task. Claim attempts may
repeat before a run is linked, but a task SHALL never replace its `run_id`. Lease
expiry alone SHALL NOT authorize a second run. Once a run exists, reconciliation
must inspect that run and use its authoritative recovery path.

Task scheduling state SHALL be limited to `queued`, `claimed`,
`cancel_requested`, and `terminal`. Task terminal reasons describe only scheduling
facts, such as cancellation before execution, accepted-identity drift, exhausted
pre-run claim attempts or required recovery. Inspection references the linked run's
typed state instead of copying it into a competing task outcome.

Claims SHALL use SQLite conditional transactions, an opaque claim token and a
database-clock lease deadline. Heartbeats and releases SHALL compare the current
claim token so a stale worker cannot mutate a newer claim. Queue depth, claim
attempts, lease duration, heartbeat interval, polling, concurrency and shutdown are
bounded host settings.

Queued cancellation SHALL become terminal without creating a run. Cancellation of
claimed work SHALL become `cancel_requested`; it is not reported as a cancelled
execution until the linked run records its own outcome. Workers are foreground-first
and explicitly started. Static inspection, servers and synchronization never start
one.

The normative behavior is defined by
[SPEC-0026](../spec/0026-durable-agent-tasks.md).

## Alternatives Considered

- **Add `queued` to `agent_runs`:** rejected because acceptance is not an execution
  attempt and current run ownership assumes execution has begun.
- **Permit a new run after every lease expiry:** rejected because an expired lease
  does not prove the previous process stopped or that its effects are safe to replay.
- **Copy run outcomes into task status:** rejected because it creates two lifecycle
  authorities that can disagree.
- **Allow multiple safe run attempts in version one:** rejected because retry safety
  cannot be inferred generically after provider or tool activity.
- **Let configuration dynamically load worker code:** rejected; hosts register
  compiled implementations explicitly and configuration only selects trusted code.
- **Ship remote submission first:** rejected because authentication and remote agent
  selection are independent public contracts.

## Consequences

- Persistence gains additive task and task-event tables without changing run rows.
- Hosts can durably accept work before a worker or model is available.
- A task may be terminal for a scheduling reason while its linked run exposes a
  suspended or non-success execution outcome; callers must inspect both layers.
- Exactly-one-run simplifies recovery and deliberately requires explicit
  resubmission when restart is required.
- Worker availability and claim leases do not weaken run locks, checkpoint identity,
  policy checks or uncertain-effect handling.
- Initial Phase 4A APIs are library/store surfaces. CLI commands and operational
  service examples remain later slices.

## References

- [PRD-0016](../prd/0016-durable-background-agent-tasks.md)
- [DESIGN-0016](../design/0016-durable-background-agent-tasks.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [ADR-0028](0028-typed-run-outcomes-and-compatibility.md)
- [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md)
- [SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md)
- [SPEC-0026](../spec/0026-durable-agent-tasks.md)
