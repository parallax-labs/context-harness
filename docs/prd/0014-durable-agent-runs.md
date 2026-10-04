# PRD-0014: Durable Long-Running Agent Runs

**Status:** Planned
**Date:** 2026-10-01
**Author:** Context Harness contributors

## Problem Statement

Context Harness already executes bounded multi-turn agent runs: a model can request
tools, observe their results and continue until it returns a final response. The
runtime also persists run records, events, tool invocations and checkpoints. This
foundation is sufficient for short agentic workflows, but it does not yet provide
the lifecycle, budget and context-management semantics users need to trust longer
tasks such as inspecting a repository, editing files, running validation, reacting
to failures and stopping with an accurate outcome.

Today, a run's model-visible working state is primarily its accumulated
conversation. Operational state is structured in SQLite, but the runtime does not
construct a purpose-specific context from the objective, plan, important
observations, artifacts and validation state on each turn. Token and monetary
budgets are not enforced across the run. Tool execution failures are generally
fatal, and outcomes such as blocked work or a request for user input collapse into
either prose marked completed or a generic failure.

Users need long-running runs to remain understandable, bounded and resumable as
their histories grow. They also need terminal status to describe what actually
happened instead of treating every non-successful outcome as the same failure.

## Target Users

- Developers using `ctx agent run` for multi-step repository investigation,
  implementation and validation.
- Agent authors defining safe tool sets, execution budgets and completion
  expectations for reusable agents.
- Operators inspecting, resuming or diagnosing durable runs after interruption or
  an external blocker.
- Host integrators that need predictable limits and typed outcomes for local or
  embedded execution.

## Goals

1. A run spanning at least 25 model/tool turns can preserve its objective, active
   work and material observations without requiring the model to consume every
   prior message verbatim.
2. Every terminal or suspended run records a typed outcome that distinguishes
   success, cancellation, failure, exhausted limits, blocking conditions and a
   request for user input.
3. Configured turn, duration, token and tool-call budgets are enforced across new
   and resumed execution without silently resetting counters.
4. Users can inspect the current plan or working summary, important observations,
   artifacts and validation state without reconstructing them from raw events.
5. Recoverable tool failures can be presented to the agent without permitting
   unbounded retries or automatic replay of uncertain side effects.
6. Existing short-running agents and the current model/tool loop remain compatible
   unless an explicit migration is documented.

## Non-Goals

- A distributed scheduler, cloud control plane, workflow DAG engine or replacement
  for Temporal-like systems.
- Indefinite autonomous execution without explicit resource limits.
- A general chat-session abstraction spanning unrelated objectives.
- Hiding or deleting the append-only execution history in favor of summaries.
- Automatic recovery of non-idempotent tool calls whose completion is uncertain.
- Defining provider pricing or guaranteeing exact monetary accounting in the first
  delivery slice.
- Changing stateless MCP prompt projection or requiring external MCP clients to use
  the local run model.
- Selecting final schemas or APIs in this PRD; those decisions belong in design,
  ADR and spec documents after review.

## User Stories

- I ask an implementation agent to inspect a repository, make a focused change,
  run tests, diagnose failures and retry validation; it continues across multiple
  tool turns and finishes only when it can report a truthful result.
- I inspect a long-running task and can quickly see its objective, current plan,
  completed work, recent important observations, modified artifacts and validation
  status without reading every raw event.
- I configure resource budgets and receive a distinct `limit_exceeded` outcome when
  one is exhausted, including which limit stopped the run and the usage consumed.
- An agent that cannot continue without a decision records `needs_user_input` with
  a concrete question rather than claiming completion.
- An agent blocked by its environment records `blocked` with actionable evidence
  and can resume after the condition changes, when recovery is safe.
- A command that exits unsuccessfully can become a model-visible observation, while
  permission violations and uncertain side effects still stop safely.
- I resume an eligible run without resetting its elapsed time, model-turn, token or
  tool-call accounting.

## Requirements

| ID | Requirement |
|---|---|
| R1 | Runs expose explicit lifecycle outcomes for completed, blocked, needs-user-input, failed, limit-exceeded and cancelled work. |
| R2 | Completion and suspension outcomes include structured, inspectable reasons rather than relying only on final prose. |
| R3 | The runtime maintains structured working state distinct from the immutable execution log and provider transcript. |
| R4 | Model context can be rebuilt from the objective, working state, relevant indexed context, recent observations, artifacts and validation results. |
| R5 | Context construction is bounded, deterministic enough to inspect, and records what categories of state were projected. |
| R6 | Turn, duration, token and tool-call usage is accumulated and enforced across the complete run lifecycle, including resume. |
| R7 | Tool failures are classified by whether they are observable/recoverable, terminal, denied or uncertain; retry policy is explicit and bounded. |
| R8 | Resume preserves safety checks for workspace, resource, model, tool, policy and completed side effects. |
| R9 | Existing runs, agent resources and provider/tool interfaces have an explicit compatibility and migration story. |
| R10 | Raw events and tool records remain the auditable history even when the model receives a compacted or synthesized context. |
| R11 | Inspection makes working state, budget consumption, termination reason and pending user input visible without exposing secrets. |
| R12 | A session abstraction is not required unless a separately reviewed use case needs continuity across multiple runs. |

## Success Criteria

- Deterministic integration tests execute a repository-style workflow through
  repeated read, patch, command and validation turns before a typed completion.
- A long synthetic run crosses the configured transcript-compaction threshold while
  retaining its objective, material observations and validation result in the model
  context and inspection output.
- Tests exhaust each supported budget independently and verify a typed
  `limit_exceeded` result with counters preserved across resume.
- Tests distinguish completed, blocked, needs-user-input, failed and cancelled
  outcomes in both durable storage and CLI JSON.
- Recoverable tool failures reach a later model turn as bounded observations;
  repeated or unsafe failures stop according to policy.
- Recovery tests prove that uncertain side effects are never replayed and completed
  runs remain immutable.
- Existing local-agent, provider, tool-policy, MCP compatibility and delegation
  suites continue to pass or have an approved migration documented in the spec.

## Dependencies and Risks

This work depends on the existing local runtime, model boundary, tool registry,
checkpoint/recovery mechanism and durable SQLite history defined by SPEC-0015
through SPEC-0022. The declarative tool bindings delivered by PRD-0013 and
SPEC-0023 determine how effective tool identity and recovery compatibility are
represented, but the working-state and lifecycle design should not depend on
application-specific tools.

Context compaction can omit evidence the model still needs, while full replay can
exhaust provider context and increase cost. Structured state can become a second,
conflicting source of truth if its relationship to append-only events is unclear.
Provider token accounting may be absent or differ by provider. Cost enforcement
can become stale as pricing changes. Recoverable errors can create loops or repeat
side effects if classification is too permissive. New statuses also require careful
migration and CLI compatibility decisions.

## Phasing

1. Define lifecycle outcomes, termination reasons and cumulative usage accounting.
2. Introduce structured working state and an inspectable context-projection seam
   that initially reproduces current transcript behavior.
3. Add bounded context selection/compaction and relevant-context retrieval.
4. Add classified recoverable tool observations and loop detection.
5. Extend resume and CLI workflows for suspended runs after their safety semantics
   are settled.

The minimum lifecycle and cumulative-budget implementation may begin under
[ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md) and
[SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md). Later working-state,
context-compaction, recoverable-error, and suspended-continuation behavior requires
its named Phase 5 contract before implementation.

## Resolved and Deferred Decisions

- ADR-0028 and SPEC-0025 define terminal and suspended outcomes, recovery
  dispositions, opt-in runtime control tools, and the legacy-row/CLI mapping.
- Initial cumulative controls are turns, duration, reported tokens, and tool calls.
  Monetary budgets remain a non-goal until pricing can be versioned reliably.
- Working-state ownership and context evidence/compaction remain deferred to Phase 5;
  the compatibility context builder continues complete valid transcript projection.
- Recoverable tool observations and loop detection remain fail-closed until the Phase
  5 contract classifies safe errors and repetition identity.
- Suspended continuation remains deferred; the first lifecycle slice records and
  exposes suspension but does not synthesize user input into provider conversation.
- No Session is introduced. Root/child lineage and resumable runs remain the product
  boundary until a separately reviewed cross-run use case requires another concept.

## Related Documents

- [DESIGN-0014](../design/0014-structured-agent-run-state.md): planning architecture,
  staging, and deferred Phase 5 decisions.
- [PRD-0013](0013-declarative-tool-bindings.md): delivered reusable runtime tool
  composition.
- [SPEC-0023](../spec/0023-declarative-tool-bindings.md): authoritative tool-binding
  contract.
- [ADR-0024](../adr/0024-local-agent-runtime.md): accepted optional local-execution
  boundary.
- [DESIGN-0010](../design/0010-local-agent-runtime.md): original runtime vision.
- [DESIGN-0011](../design/0011-local-agent-runtime-execution-plan.md): implemented
  bounded runtime foundation.
- [SPEC-0017](../spec/0017-local-agent-execution.md): current execution contract.
- [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md): current recovery
  and artifact contract.
- [ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md): lifecycle,
  compatibility, control-call, and accounting architecture.
- [SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md): authoritative
  lifecycle, budget, inspection, and recovery contract.
- A future ADR/spec is still required before working-state ownership, compaction,
  recoverable tool observations, or suspended continuation changes runtime behavior.
