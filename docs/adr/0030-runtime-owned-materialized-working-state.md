# ADR-0030: Runtime-Owned Materialized Working State

**Status:** Accepted  
**Date:** 2026-10-06

## Context

PRD-0014 requires long-running agent runs to expose useful working state without
making the provider transcript the only cognitive state or replacing append-only
execution history. The runtime already persists canonical run, event, tool,
artifact, checkpoint, lifecycle and usage records. DESIGN-0014 left two choices
open: the shape of the working-state representation and which actor may update it.

A free-form model scratchpad would be easy to produce but difficult to validate,
rebuild or trust. Deriving every inspection from the full event log would preserve
one source of truth but make bounded inspection increasingly expensive. A new set
of normalized planning and observation tables would prematurely define semantics
needed by later context-selection and model-authored-state work.

## Decision

Each run has one versioned, revisioned materialized working-state snapshot. The
snapshot is a workspace-scoped projection of deterministic runtime facts and
references to canonical records. It does not copy unrestricted model messages,
tool arguments/results, event payloads, credential values or provider errors.

Append-only events and the existing run, tool, artifact and checkpoint records
remain canonical. Every snapshot update commits in the same transaction as the
canonical event that caused it. The snapshot records the event cursor and can be
rebuilt from canonical records; it is not an independent source of lifecycle,
usage, artifact or execution truth.

The runtime is the only Phase 5A writer. Models and tools cannot mutate working
state directly. Model-authored plans, summaries, observations or importance labels
require a later constrained control-call contract with validation, provenance and
budget rules.

Phase 5A does not use working state to select, omit, summarize or rewrite model
context. The compatibility context builder and checkpoint request remain unchanged.

## Alternatives Considered

### Derive the complete projection on every read

This avoids a materialized row but makes inspection cost grow with event history and
does not establish the transactional projection boundary needed by later context
construction.

### Normalize plans, observations and validation into separate tables

This supports rich queries but fixes semantics that Phase 5B and 5C have not yet
specified. It also risks duplicating canonical tool, event and artifact records.

### Let the model own a free-form snapshot

This provides apparent flexibility but makes state untrusted, difficult to bound and
prone to contradicting durable execution facts.

### Store working state only in checkpoints

Checkpoints are recovery snapshots, may be absent for non-resumable runs and are not
the canonical inspection surface. Coupling inspection to them would conflate
execution recovery with materialized run state.

## Consequences

- Inspection receives a bounded, stable working-state surface without replaying the
  full event log.
- Runtime facts remain auditable against canonical records and event sequence.
- Updates add transactional work to event-producing store operations.
- The projection must have rebuild and corruption-detection tests.
- Phase 5A cannot expose a model-authored plan or summary; later work must introduce
  an explicit, constrained mutation contract.
- Context compaction, recoverable tool observations and suspended continuation remain
  separate decisions.

## References

- [PRD-0014](../prd/0014-durable-agent-runs.md)
- [DESIGN-0014](../design/0014-structured-agent-run-state.md)
- [SPEC-0027](../spec/0027-structured-agent-working-state.md)
- [ADR-0028](0028-typed-run-outcomes-and-compatibility.md)

