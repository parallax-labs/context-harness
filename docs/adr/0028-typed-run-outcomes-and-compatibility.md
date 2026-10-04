# ADR-0028: Typed Run Outcomes with a Legacy Status Projection

**Status:** Accepted
**Date:** 2026-10-03

## Context

Context Harness persists one of four strings in `agent_runs.status`: `running`,
`completed`, `failed`, or `cancelled`. That representation is sufficient for the
current direct runtime, but it conflates execution activity, the meaning of a stop,
and whether recovery is safe. In particular, it cannot distinguish an environmental
blocker, a request for user input, an exhausted budget, an ordinary failure, or an
uncertain side effect.

Background-task reconciliation needs one authoritative execution vocabulary before it
can project run results. Existing databases, CLI consumers, checkpoints, and library
callers nevertheless depend on the four status strings. Replacing that field or
reinterpreting `completed` would make old readers report incorrect success or fail to
read new rows.

The model also needs a provider-neutral way to report blocked and needs-input
outcomes. Requiring structured final output would couple lifecycle control to provider
schema support. Treating prose as control data would be ambiguous and unsafe.

## Decision

Canonical run state SHALL separate three concepts:

1. **lifecycle state:** `active`, `suspended`, or `terminal`;
2. **outcome:** `completed`, `blocked`, `needs_user_input`, `failed`,
   `limit_exceeded`, or `cancelled`, absent while active; and
3. **reason:** a typed, sanitized code plus bounded structured detail.

The existing `status` field SHALL remain as a compatibility projection. Active runs
project to `running`; completed and cancelled outcomes retain their existing values;
all other stopped outcomes project to `failed`. New code SHALL use the canonical
fields. Old readers therefore continue to recognize every row and never mistake a
non-success outcome for success.

`blocked` and `needs_user_input` SHALL be suspended outcomes. `completed`, `failed`,
`limit_exceeded`, and `cancelled` SHALL be terminal outcomes. Recovery eligibility is
an independently derived safety decision, not a synonym for either lifecycle or
outcome. The first implementation SHALL preserve existing failed/cancelled recovery,
identify suspended runs for hosts and future workers, and defer continuation of a
suspended conversation until the checkpoint/input protocol is implemented.

Ordinary final model text SHALL remain backward-compatible completion. An agent may
select a suspended outcome only by explicitly declaring and invoking reserved,
runtime-owned control tools. Control calls are typed, have no external capability,
cannot be overridden by a host tool, and terminate the current execution without an
ordinary tool result turn. Existing agents that do not declare them keep their current
tool schemas and behavior.

Cumulative turns, elapsed duration, reported tokens, and started tool calls SHALL be
canonical run usage. Missing provider token usage SHALL remain unknown. If a token
budget is configured and a response omits usage, execution SHALL stop conservatively
rather than treating the missing count as zero. Budgets and usage SHALL persist across
resume and delegation boundaries as defined by the normative specification.

The normative behavior is defined by
[SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md).

## Alternatives Considered

- **Replace `status` with the expanded outcome enum:** simpler internally, but old
  SQLite constraints and callers cannot read new strings safely.
- **Encode every distinction in one status enum:** still conflates activity,
  execution meaning, and recovery safety; it also makes future recovery changes into
  schema vocabulary changes.
- **Map blocked and needs-input to completed:** preserves a zero CLI exit but falsely
  reports unfinished work as success.
- **Infer lifecycle from final prose:** provider-neutral but nondeterministic and
  vulnerable to incidental wording.
- **Require structured model output:** precise for providers that support it, but not
  portable across the supported model catalog and disruptive to ordinary outputs.
- **Advertise control tools to every agent:** makes the feature immediately available,
  but changes request identity and model behavior for resources that did not opt in.
- **Treat missing token usage as zero:** allows execution to exceed an explicitly
  configured budget without detection.

## Consequences

- Persistence needs additive canonical lifecycle, outcome, reason, budget, and usage
  fields while retaining the legacy status projection.
- Store, event, inspection, and host APIs gain typed public representations; arbitrary
  status strings remain unsupported.
- Background tasks can project canonical outcomes without defining a second execution
  state machine.
- Existing CLI and JSON consumers keep their known status values, although new
  consumers should prefer canonical fields.
- Suspended outcomes can ship before conversation continuation; inspection must make
  that limitation explicit.
- Token-budgeted execution fails closed when a provider cannot supply usage, which can
  reject an otherwise usable final response.
- Context compaction, model-authored working state, recoverable tool observations, and
  application-specific behavior remain outside this decision.

## References

- [PRD-0014](../prd/0014-durable-agent-runs.md)
- [DESIGN-0014](../design/0014-structured-agent-run-state.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [SPEC-0017](../spec/0017-local-agent-execution.md)
- [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md)
- [SPEC-0021](../spec/0021-agent-delegation.md)
- [SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md)
