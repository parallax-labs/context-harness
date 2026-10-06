# SPEC-0027: Structured Agent Working State

**Status:** Implemented

**Date:** 2026-10-06  
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md),
[DESIGN-0014](../design/0014-structured-agent-run-state.md),
[ADR-0030](../adr/0030-runtime-owned-materialized-working-state.md),
[SPEC-0017](0017-local-agent-execution.md),
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md),
[SPEC-0025](0025-durable-run-lifecycle-and-budgets.md)

## Scope

This specification defines the first structured working-state representation for
durable agent runs, its authority, transactional relationship to append-only events,
workspace isolation, public inspection and compatibility behavior.

It does not define model-authored plans or summaries, observation importance,
validation semantics, context selection or compaction, recoverable tool failures,
suspended continuation, or a session abstraction.

## Definitions

- **Canonical records:** the existing run, event, tool-invocation, artifact,
  checkpoint and lineage records whose contracts define execution truth.
- **Working-state snapshot:** one bounded, materialized projection per run containing
  deterministic runtime facts and references to canonical records.
- **Revision:** a positive, monotonically increasing snapshot version local to one
  run.
- **Event cursor:** the greatest run-event sequence incorporated into the snapshot.
- **Projection version:** the schema and derivation algorithm version for the
  snapshot body.

## Authority and ownership

The runtime SHALL be the only writer of Phase 5A working state. Model responses,
ordinary tools, compiled hosts and configuration SHALL NOT directly supply or mutate
snapshot fields. No tool or model-control declaration for working-state mutation is
introduced by this contract.

Canonical records SHALL remain authoritative. A snapshot SHALL NOT override run
lifecycle, outcome, reason, usage, tool status, artifacts, lineage or checkpoint
state. Readers that detect a missing, malformed, unsupported or inconsistent
snapshot SHALL report the corresponding typed inspection status while continuing
to report canonical run data.

## Persistence

The application database SHALL contain at most one working-state row per run, bound
through the run identifier and its workspace-scoped parent record. The row SHALL
contain:

- `run_id`;
- positive `projection_version`;
- positive, monotonically increasing `revision`;
- non-negative `event_cursor` not greater than the run's `last_sequence`;
- `updated_at` as Unix milliseconds; and
- a JSON snapshot satisfying the versioned schema below.

The initial row SHALL be created in the same transaction as the run and its first
event. Every subsequent run event SHALL update the snapshot in the same transaction
because version 1 projects the latest event reference. Its event cursor SHALL equal
that event's sequence. A canonical transaction that changes a projected fact without
adding an event to that run, including shared ancestor usage charged by a delegated
child, SHALL update the affected run's snapshot in the same transaction while leaving
its event cursor unchanged. The revision SHALL increase exactly once for each such
transaction affecting the snapshot.

A transaction SHALL NOT commit only one of the canonical event and required snapshot
update. Failed snapshot serialization, bounds validation or persistence SHALL roll
back both. Concurrent updates SHALL preserve the existing gapless event ordering and
produce a snapshot for the greatest committed cursor; an older transaction SHALL NOT
overwrite a newer revision.

## Projection version 1

The version 1 snapshot SHALL be a JSON object containing exactly these top-level
members:

```json
{
  "objective": {"kind": "run_input", "run_id": "<run-id>"},
  "run": {
    "lifecycle": "active | suspended | terminal",
    "outcome": null,
    "reason_code": null
  },
  "usage": {
    "model_turns": 0,
    "input_tokens": null,
    "output_tokens": null,
    "total_tokens": null,
    "responses_with_usage": 0,
    "responses_without_usage": 0,
    "tool_calls": 0,
    "token_accounting": "unavailable"
  },
  "latest_event": {"sequence": 1, "event_type": "run.started"},
  "artifacts": {"items": [], "total": 0, "omitted": 0}
}
```

`objective` SHALL reference `agent_runs.input`; it SHALL NOT duplicate the objective
text. `run` and `usage` SHALL equal the canonical materialized run values at the
snapshot cursor. `outcome` and `reason_code` SHALL preserve `null` for an active run.
`latest_event` SHALL contain only its sequence and type, never its payload.

`artifacts.items` SHALL contain at most the 128 most recent canonical artifact
references. Each member SHALL contain exactly `sequence`, `relative_path`, `sha256`
and `size`, equal to an existing `artifact.created` record. Selection SHALL retain
the highest event sequences; retained items SHALL be presented by sequence and then
relative path in ascending order. `total` SHALL equal the number of canonical
artifact records through the cursor and `omitted` SHALL equal
`total - items.length`, so the bound never silently hides that older references
exist. No snapshot field SHALL contain raw model messages, reasoning items, tool
arguments/results, event payloads, unrestricted provider/tool errors, claim tokens
or credential values.

The serialized snapshot SHALL be at most 256 KiB. An update that would exceed the
bound after applying the explicit 128-reference artifact window SHALL fail closed and
roll back its canonical event transaction. Projection version 1 SHALL NOT truncate
any other field.

## Deterministic derivation and rebuild

Given the same canonical records through the same event cursor, projection version 1
SHALL produce byte-equivalent canonical JSON. Object keys SHALL use the declared
schema and artifact ordering SHALL follow this specification.

The store SHALL provide a trusted rebuild operation that replaces a missing, stale
or corrupt snapshot from canonical records without invoking a model or tool. Rebuild
SHALL operate inside one database transaction, preserve canonical records, allocate
no run event and set the cursor to the run's current `last_sequence`. A rebuild SHALL
increment the existing row revision with checked arithmetic, even when its JSON body
is corrupt, or use revision 1 when no row exists.

Normal read-only inspection SHALL NOT rebuild, migrate or write. It SHALL expose
whether the snapshot is `current`, `stale`, `unavailable` or `unsupported` by comparing
its cursor and projection version with canonical run state.

`current` means a valid supported snapshot whose cursor equals the run's
`last_sequence`. `stale` means a valid supported snapshot whose cursor is lower.
`unsupported` means the row declares a projection version the reader does not
understand. A missing, malformed or internally inconsistent row, including a cursor
greater than `last_sequence`, SHALL be `unavailable`.

## Public store and CLI inspection

Public run-store APIs SHALL expose a typed working-state projection containing the
row metadata, version 1 snapshot and inspection status. Reads SHALL be workspace
scoped and SHALL NOT return a snapshot for a run owned by another workspace.

`ctx agent inspect <run-id> --json` SHALL add a `working_state` member without
removing or changing existing fields. The member SHALL contain the inspection status
and, when supported and valid, the typed snapshot. Human inspection SHALL print the
status, projection version, revision, event cursor and bounded referenced facts; it
SHALL NOT print content that the JSON projection forbids.

History output SHALL remain unchanged in Phase 5A. Task inspection SHALL continue to
project its linked canonical run and SHALL NOT duplicate working state in the task
record.

Working-state inspection SHALL use only the existing database. It SHALL NOT load
agent or tool resources, construct a provider, resolve secrets, invoke a model/tool,
access the network, evaluate Lua, start MCP, spawn a process, acquire an execution
lock or create/migrate a missing database.

## Runtime and checkpoint compatibility

Phase 5A SHALL NOT change `ModelRequest`, provider transcript construction,
`CompatibilityContextBuilder`, projection categories, tool declarations or model-call
ordering. Working state SHALL NOT be sent to the provider or used to omit, reorder or
summarize messages under this contract.

The checkpoint schema and binding identity SHALL remain unchanged because version 1
working state is deterministic and rebuildable from canonical records. Resume SHALL
continue to validate and restore the existing checkpoint request. It MAY update the
materialized snapshot only as part of later canonical event transactions and SHALL
never restore an older snapshot over a newer committed event cursor.

Legacy databases and runs without working-state rows SHALL remain readable and
resumable under their existing contracts. Additive migration SHALL create only the
new table/indexes; it SHALL NOT synthesize rows, rewrite events/checkpoints or invent
historical working state. New execution or an explicit trusted rebuild MAY create the
first snapshot for a legacy run.

## Acceptance Criteria

- Creation tests prove the run, first event and revision-1 snapshot commit atomically.
- Transition, direct/delegated usage and artifact tests prove canonical records and
  required snapshot revisions commit or roll back together.
- Concurrency tests prove revisions and cursors never regress and the final snapshot
  matches the greatest committed event sequence.
- Rebuild tests prove byte-equivalent projection from canonical records without a
  new event, provider/tool construction or external side effect.
- Corrupt, missing, stale and future-version rows return typed inspection states
  without hiding canonical run data or writing during inspection.
- Workspace tests prove snapshots cannot cross workspace boundaries.
- CLI JSON and human tests expose the bounded projection without secrets, raw
  payloads or compatibility-field changes.
- Existing runtime request, checkpoint, resume, provider, tool, delegation, task and
  run-history fixtures remain unchanged.
