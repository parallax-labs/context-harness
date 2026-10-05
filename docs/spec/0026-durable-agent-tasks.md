# SPEC-0026: Durable Agent Tasks

**Status:** Authoritative — implementation pending
**Date:** 2026-10-05
**Related:** [PRD-0016](../prd/0016-durable-background-agent-tasks.md),
[DESIGN-0016](../design/0016-durable-background-agent-tasks.md),
[ADR-0029](../adr/0029-durable-agent-task-ownership.md),
[SPEC-0017](0017-local-agent-execution.md),
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md),
[SPEC-0023](0023-declarative-tool-bindings.md),
[SPEC-0025](0025-durable-run-lifecycle-and-budgets.md)

## Scope

This specification defines local durable task submission, identity, persistence,
queue bounds, worker claims and leases, cancellation, task-to-run linkage,
reconciliation, inspection and trusted host APIs. A task represents accepted intent;
a run represents its single execution attempt.

This specification does not define distributed scheduling, priorities, cron,
dependencies, DAGs, remote unauthenticated submission, multiple run attempts,
suspended-run continuation, application job states, hooks, publication, context
compaction or automatic worker startup.

## Definitions

- **Task:** durable accepted intent to run one named standalone agent with immutable
  input in one workspace.
- **Request key:** caller-selected idempotency key unique within one workspace.
- **Request digest:** versioned SHA-256 identity of immutable task input.
- **Accepted identity:** versioned, non-secret identity of the resource, model,
  provider implementation, tool bindings and host policy accepted at submission.
- **Claim:** temporary scheduling ownership held by one worker using an opaque token.
- **Lease:** database-clock deadline after which a claim may be reconciled.
- **Claim attempt:** a successful transition from queued to claimed before any run is
  linked.
- **Run link:** immutable association from a task to its only `agent_runs` row.
- **Scheduling reason:** typed explanation of a terminal task transition; it is not a
  run outcome.

## Task identity and submission

Submission SHALL accept:

- a nonempty agent resource name;
- nonempty UTF-8 input of at most 64 KiB;
- a UTF-8 request key of 1 through 256 bytes;
- an optional opaque host payload identity of at most 8 KiB; and
- a positive queue limit no greater than 10,000, defaulting to 1,000.

The host payload identity is identity-only data. The framework SHALL store it but
SHALL NOT interpret it as an application event, command, hook or policy. It SHALL NOT
contain credential values. Arbitrary executable configuration is invalid.

The request digest SHALL be lowercase hexadecimal SHA-256 over this unambiguous byte
sequence:

```text
"context-harness.agent-task.request.v1\0"
u64_be(agent_name_len) || agent_name_utf8
u64_be(input_len) || input_utf8
u64_be(payload_identity_len) || payload_identity_bytes
```

The request key is not part of the digest. Accepted identity is compared separately
and SHALL NOT be silently replaced on a duplicate submission.

Before writing, submission SHALL statically resolve the named agent through the
trusted `AgentHostBuilder` inputs and compute accepted identity. It SHALL validate
the resource, reachable delegation resources, model definitions, provider factory
identity, tool declarations, selected binding metadata and host policy without
constructing a provider, resolving a credential, opening a network connection,
starting MCP or another process, evaluating Lua, invoking a model or executing a
tool.

Accepted identity SHALL contain a schema version and the canonical serialization of:

- canonical workspace ID;
- root agent name and content-derived resource version;
- reachable agent names and resource versions in lexical order;
- each referenced model alias, configured provider/model name and registered
  provider implementation ID/version;
- advertised tool schemas and selected non-secret binding metadata;
- the binding contract version; and
- host permission ceiling and approval requirements.

Maps and sets SHALL serialize in lexical key order. The stored identity SHALL include
the structured non-secret components and their lowercase hexadecimal SHA-256 digest.
Secret values, resolved environment values and raw provider/tool errors SHALL never
enter either representation.

Submission SHALL atomically enforce workspace-scoped request-key uniqueness and the
queue limit:

- no existing key and queued count below the limit: insert and return a new task;
- same key, request digest and accepted-identity digest: return the existing task
  without mutation;
- same key with either digest changed: return `request_conflict` without mutation;
- queued count at the limit: return `queue_full` without mutation.

The queue count SHALL include `queued`, `claimed` and `cancel_requested` tasks and
exclude `terminal` tasks. Contending identical submissions SHALL return one task ID.
Submission SHALL commit before returning and SHALL perform no model or tool work.

## Persistence

An additive migration SHALL create `agent_tasks` with at least:

```text
id, workspace_id, request_key, request_digest,
agent_name, agent_version, accepted_identity, accepted_identity_digest,
input, payload_identity,
status, scheduling_reason, scheduling_detail,
claim_owner, claim_token, lease_expires_at, claim_attempts,
run_id, created_at, updated_at, claimed_at, completed_at
```

Task IDs SHALL be UUIDs. `(workspace_id, request_key)` SHALL be unique. `run_id`, when
present, SHALL reference `agent_runs(id)` and SHALL never change. Timestamps are Unix
milliseconds. Structured identity and detail fields SHALL be valid JSON. Scheduling
detail SHALL be a JSON object no larger than 8 KiB and SHALL contain no credentials,
raw provider errors or unrestricted tool/model payloads.

An additive `agent_task_events` table SHALL record workspace-scoped, gapless,
task-local sequence numbers. Every state transition and its event SHALL commit in one
transaction. Initial event names SHALL be:

```text
task.submitted
task.claimed
task.heartbeat
task.run_linked
task.cancel_requested
task.requeued
task.terminal
```

Heartbeat events MAY be coalesced, but the materialized lease SHALL still update
transactionally. Existing run, event, checkpoint, approval, invocation and artifact
tables SHALL remain unchanged.

## Scheduling states and reasons

Valid scheduling states are:

| State | Meaning |
|---|---|
| `queued` | available for a future claim; no worker owns it |
| `claimed` | one worker owns the current claim |
| `cancel_requested` | claimed work has a durable cancellation request |
| `terminal` | no further scheduling transition is automatic |

`queued` SHALL have no claim fields. `claimed` and `cancel_requested` SHALL have a
claim owner, claim token and lease deadline. `terminal` SHALL have no active claim.
A task with a linked run SHALL never return to `queued`.

Terminal scheduling reasons are limited to:

```text
cancelled_before_run
run_stopped
accepted_identity_changed
claim_attempts_exhausted
restart_required
reconciliation_required
```

`run_stopped` does not describe success or failure. Inspection SHALL obtain the
linked run lifecycle, outcome, reason and recovery disposition from the run store.
Unknown future scheduling reasons SHALL be preserved and treated as non-success by
older readers.

## Claims, leases and worker bounds

Worker options SHALL be explicit, positive and bounded:

| Setting | Default | Allowed range |
|---|---:|---:|
| concurrency | 1 | 1–32 |
| lease duration | 60 s | 5–3600 s |
| heartbeat interval | 20 s | 1 s through less than half the lease |
| polling interval | 500 ms | 50 ms–60 s |
| maximum pre-run claim attempts | 3 | 1–100 |
| graceful shutdown timeout | 30 s | 1–3600 s |

A worker ID SHALL be caller-supplied or generated at worker startup, contain 1 through
128 printable non-control UTF-8 bytes, and identify only that worker lifetime. Every
successful claim SHALL generate an unpredictable UUID claim token.

Claiming SHALL use one SQLite write transaction and the database clock. It SHALL
select the oldest eligible queued task by `(created_at, id)`, conditionally transition
it to `claimed`, increment `claim_attempts`, set owner/token/deadline and append
`task.claimed`. No two workers may successfully claim the same transition.

Heartbeats SHALL extend from the database's current time, not from the prior deadline,
and SHALL succeed only when task ID, workspace, claim owner, claim token and current
claimed or cancel-requested state all match. A stale worker SHALL be unable to extend,
link, requeue, cancel-finalize or complete a newer claim.

Worker concurrency limits active task executions, including reconciliation. Polling
SHALL stop when shutdown is requested. Graceful shutdown SHALL request cancellation
for runtime executions owned by that worker, continue heartbeat while awaiting them,
and stop after the configured timeout without claiming new work. Process exit alone
does not change durable task or run state.

## Accepted identity and run linkage

After claim and before run creation, the worker SHALL recompute current accepted
identity through the same static resolver. A mismatch SHALL transition the task to
terminal `accepted_identity_changed` without creating a run or constructing a
provider. The operator must submit a new request key or explicitly cancel the old
intent; the task SHALL NOT silently adopt broader or different authority.

Exactly one run may be linked to a task. Run creation and `task.run_linked` SHALL
commit atomically in the same SQLite database before provider construction, model
invocation or tool execution. The linked run SHALL use the accepted agent resource,
ordinary run budgets and existing runtime ownership. Subsequent execution SHALL use
the same `AgentRuntime` path as direct execution.

The task store SHALL NOT copy model messages, tool history, run usage, artifacts,
checkpoints or run outcomes. A task with `run_id` SHALL never clear or replace it,
even when its lease expires or execution cannot resume.

## Cancellation

Cancellation SHALL be workspace-scoped and idempotent.

- `queued` without a run transitions atomically to terminal
  `cancelled_before_run`.
- `claimed` transitions atomically to `cancel_requested` while retaining the current
  claim.
- repeated cancellation of `cancel_requested` changes nothing.
- cancellation of `terminal` returns the stored task unchanged.

A worker observing `cancel_requested` SHALL signal its owned runtime cancellation
channel when a run is executing. The task SHALL remain nonterminal until the linked
run records a stopped outcome or reconciliation selects a terminal scheduling reason.
Task cancellation SHALL NOT directly rewrite run lifecycle or outcome.

If cancellation is requested before run creation, the owner SHALL finalize terminal
`cancelled_before_run` and SHALL NOT create a run. If run creation and cancellation
contend, their conditional transaction order decides whether a run exists; either
result remains inspectable and no second run may be created.

## Reconciliation

Lease expiry authorizes reconciliation of scheduling ownership, never replay or a new
run by itself. A reconciler SHALL acquire a fresh conditional claim token before
mutating a task and SHALL follow this matrix:

| Persisted state | Required action |
|---|---|
| no run, attempts remain, no cancellation | requeue and permit another pre-run claim |
| no run, attempts exhausted | terminal `claim_attempts_exhausted` |
| no run, cancellation requested | terminal `cancelled_before_run` |
| linked run `complete` | terminal `run_stopped` |
| linked run `suspended` | terminal `run_stopped`; retain suspension for explicit future continuation |
| linked run `resume_eligible` | invoke authoritative `AgentRuntime::resume` for that same run |
| linked run `restart_required` | terminal `restart_required`; never create another run |
| linked run `reconciliation_required` | terminal `reconciliation_required`; never replay effects |

If authoritative resume rejects because the run still has an active execution owner,
the reconciler SHALL retain its task claim, continue heartbeating and observe the
linked run without invoking it. It MAY stop on graceful shutdown and allow another
reconciler to repeat that observation after lease expiry. If resume instead rejects
resource, provider, binding, policy or checkpoint validation, it SHALL leave run
history unchanged and the task SHALL become terminal `restart_required`, unless
durable inspection reports `reconciliation_required`, which takes precedence. A
stale claim token prevents the original worker from rewriting task scheduling state;
later inspection/reconciliation always projects the stored run truth.

Worker crash injection around claim, run linkage, model request, tool start,
checkpoint and terminal projection SHALL never produce a second run ID or replay an
uncertain invocation.

## Inspection and public host APIs

`AgentTaskStore` SHALL be a public, workspace-scoped persistence facade over the
application's existing SQLite pool. Phase 4A SHALL expose typed submission,
get/list/event inspection and queued cancellation. Submission results SHALL
distinguish created, existing-identical, request-conflict and queue-full outcomes
without parsing error prose.

Task inspection SHALL expose scheduling fields, accepted identity, linked run ID and,
when linked, the current typed run state/outcome/reason/recovery disposition. It SHALL
not flatten these into one status. Listing SHALL be bounded, deterministic and
workspace-scoped.

`AgentHostBuilder` SHALL gain task submission and worker assembly only through
additive APIs. Builder configuration remains inert. Providers and executable tools
are registered explicitly by trusted host code; task configuration cannot dynamically
load code. Two unrelated public-API fixtures SHALL prove registration, submission and
task/run correlation without private modules or provider/tool name dispatch.

No CLI task surface is required for Phase 4A. Enqueue, worker, list, inspect and
cancel commands, if shipped, SHALL be a later Phase 4D slice against this contract.
`ctx serve`, sync, list/show/validate and ordinary direct-run commands SHALL never
start a worker implicitly.

Read-only task inspection SHALL access only the existing database. Static agent/tool
inspection remains database-free. Neither path may construct a provider, resolve a
secret, invoke a model/tool, access the network, evaluate Lua, start MCP or spawn a
process. Persisted or printed diagnostics SHALL use typed sanitized codes and SHALL
never include credential values or raw provider/tool errors.

## Compatibility

The migration is additive. Existing databases without task tables SHALL initialize
them without rewriting run rows, events, checkpoints or artifacts. Existing direct
run, resume, history, profiles, bindings, delegation, MCP and targeted-refresh
behavior remains unchanged. Read-only access to a pre-task database SHALL continue to
inspect runs without requiring task migration.

## Acceptance Criteria

- Migration fixtures upgrade representative historical databases without changing
  run history and are idempotent.
- Concurrent identical submissions return one task; changed input, payload or
  accepted identity under the same key conflicts transactionally.
- Queue bounds count every nonterminal scheduling state and reject before insertion.
- Submission and identity validation tests prove no provider build, credential
  resolution, database-independent execution, model call, network access or process
  startup occurs.
- Contention tests prove only one claim token owns a task and stale workers cannot
  heartbeat, link, requeue or finalize it.
- Cancellation tests cover queued, claimed, linked, repeated and race behavior
  without optimistic run cancellation.
- Crash/reconciliation tests cover every matrix row and never create a second run or
  replay an uncertain side effect.
- Public host fixtures correlate request key, task ID and run ID while retaining
  explicit authority.
- Direct runtime and recovery suites remain unchanged and green.
