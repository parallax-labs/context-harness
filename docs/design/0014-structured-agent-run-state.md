# DESIGN-0014: Structured Agent Run State and Context Projection

**Status:** Planning
**Date:** 2026-10-01
**Author:** Context Harness contributors
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md), [ADR-0024](../adr/0024-local-agent-runtime.md), [ADR-0030](../adr/0030-runtime-owned-materialized-working-state.md), [SPEC-0017](../spec/0017-local-agent-execution.md), [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md), [SPEC-0027](../spec/0027-structured-agent-working-state.md)

## Context

The current runtime is already temporally agentic. `AgentRuntime::execute` loops
over model calls, records assistant tool requests, executes tools, appends tool
results to `ModelRequest.messages` and invokes the model again. It persists a
materialized run, ordered events, tool invocations, artifacts and versioned
checkpoints. Model completion, turn exhaustion, timeout, cancellation and fatal
errors terminate the loop.

The next problem is not adding a loop. It is making longer runs bounded,
inspectable and semantically accurate. The current model-visible state is chiefly
an ever-growing provider conversation, while richer operational facts live in
separate persistence records. Status is limited to running, completed, failed and
cancelled. Provider usage is recorded per response but is not enforced as a
cumulative budget. Tool implementation errors are fatal rather than classified
observations. A model can describe itself as blocked in prose, but the durable run
will still be completed if the provider reports a normal final response.

This document keeps later working-state and context-quality architecture exploratory.
The minimum lifecycle and cumulative-budget boundary has graduated to ADR-0028 and
SPEC-0025; those documents are authoritative where this design's earlier alternatives
or tentative vocabulary differ.

## Design Principles

1. Preserve the existing model/tool execution loop and generic `ToolRegistry`
   dispatch unless evidence shows they are insufficient.
2. Keep append-only events and durable tool records as the audit history.
3. Treat model context as a bounded projection of run state, not as canonical
   storage for the run.
4. Make every limit and lifecycle transition inspectable and deterministic.
5. Fail closed around permissions, uncertain side effects and incompatible resume.
6. Add no Session abstraction until a cross-run workflow demonstrates its value.
7. Introduce compatibility seams before changing current behavior.

## Current Architecture

```text
CLI
  -> resource and model resolution
  -> AgentRuntime::run
  -> durable AgentRun
  -> AgentRuntime::execute
       -> checkpoint ModelRequest
       -> invoke model
       -> final response: complete
       -> tool calls: validate, authorize, execute and persist
       -> append assistant call plus tool results to ModelRequest
       -> invoke model again
```

Persistence separates materialized run state, ordered events, tool invocations,
checkpoints, lineage and artifacts. Checkpoints nevertheless carry the complete
`ModelRequest`, making accumulated conversation the primary resumable cognitive
state. This is appropriate for the initial runtime but does not provide explicit
planning, observation importance, validation state or context compaction.

## Proposal

### 1. Add a context-projection seam before changing context behavior

Extract initial request construction and subsequent-turn request preparation from
`AgentRuntime::execute` behind an internal interface provisionally called
`RunContextBuilder`:

```rust
trait RunContextBuilder {
    async fn build(
        &self,
        state: &RunState,
        recent: &[AgentEvent],
        tools: &[ModelTool],
        budget: &ContextBudget,
    ) -> Result<ProjectedContext>;
}
```

The first implementation should reproduce current behavior: static system prompt,
objective, complete valid conversation and tool declarations. This makes the
architectural boundary testable before adding selection or summarization.

`ProjectedContext` should contain the `ModelRequest` plus non-secret projection
metadata suitable for inspection, such as included state categories, event cursor,
estimated size and compaction strategy. It should not persist raw duplicate content
in ordinary events.

### 2. Separate canonical run facts from model working state

The tentative state model is:

```text
Run
├── identity and immutable objective
├── lifecycle status and termination reason
├── cumulative budgets and usage
├── working state
│   ├── current plan or summary
│   ├── completed work
│   ├── open questions
│   └── pending user-input request
├── observations
│   ├── source/tool reference
│   ├── importance and freshness
│   └── bounded content or artifact reference
├── tool executions
├── artifacts and modified paths
├── validation results
└── append-only events
```

Not every item requires a new table. Immutable facts already represented by tool,
artifact and event records should remain there. A materialized working-state record
can reference those facts and be updated transactionally with an event. The design
must specify whether working state is model-authored, runtime-derived or both before
its schema is finalized.

### 3. Preserve audit history while projecting selective context

Per-turn context should be able to combine:

```text
stable context
  objective, system instructions, permissions, available tools

active working context
  plan, completed work, blockers, open questions, validation state

retrieved context
  repository or indexed knowledge relevant to the next decision

recent context
  latest model/tool exchanges, with the newest observation retained in full

compressed history
  summaries and references for older observations and artifacts
```

The projection must be reconstructible enough to explain why information was
included or omitted. Raw execution history remains available for inspection and
recovery even when it is not sent to the provider.

The initial compaction mechanism is unresolved. Candidates include deterministic
truncation/selection, a model-generated rolling summary, retrieval over prior run
observations, or a hybrid. Any model-generated summary must be treated as derived
state and remain traceable to source events.

### 4. Introduce typed lifecycle and termination reasons

ADR-0028 and SPEC-0025 resolve the minimum lifecycle as:

```text
active:
  running

suspended:
  blocked
  needs_user_input

terminal:
  completed
  failed
  limit_exceeded
  cancelled
```

Lifecycle is paired with a typed outcome, reason, and independently derived recovery
disposition. The existing four-value status remains a compatibility projection. A
normal final response remains completion; opt-in runtime-owned `run.blocked` and
`run.request_user_input` controls express suspension. Suspended continuation remains
deferred rather than implied by those controls.

### 5. Make cumulative budgets first-class run state

The runtime already enforces model-turn and wall-clock limits. Extend the budget
model tentatively with:

```text
max_turns
max_duration
max_total_tokens
max_tool_calls
max_tool_calls_per_turn
max_context_tokens
repeated_identical_call_threshold
tool_failure_threshold
max_cost                     # later, if pricing can be reliable
```

Usage should be accumulated transactionally from provider responses and tool
transitions. Resume must derive or restore counters without resetting them.
Provider-reported usage can be exact when available; the fallback and enforcement
semantics when it is absent remain open.

Turn, duration and tool-call limits can be provider-neutral. Monetary limits couple
runtime behavior to mutable pricing data and should be deferred until the project
has a credible pricing/versioning source.

### 6. Classify tool outcomes before allowing recovery

The current conservative rule makes tool implementation errors fatal. Preserve
that default while defining a typed failure taxonomy:

```text
completed
denied
recoverable_error
terminal_error
uncertain_side_effect
cancelled
```

A recoverable error may become a bounded model-visible observation. Permission
denial, argument rejection and uncertain non-idempotent execution should remain
fail-closed unless an authoritative spec defines another safe outcome. Normal
process exit failures should remain structured command results rather than runtime
transport failures.

Loop safeguards should consider repeated call identity, normalized arguments,
failure category and consecutive failures. Counting failures alone is insufficient
because a sequence of distinct diagnostic attempts may be legitimate.

### 7. Evolve checkpoints after the state contract is settled

Checkpoints should eventually preserve canonical run state plus enough provider
conversation/continuation data to rebuild a valid request. They should not require
the provider transcript to be the only representation of working state.

Recovery must continue validating:

- workspace and run ownership;
- agent resource and model binding;
- effective tool and host-policy binding;
- consumed budgets and original deadline;
- completed tool results;
- absence of uncertain activity after the recovery boundary.

Support for external MCP sessions and delegated run trees should remain deferred
until their session and shared-budget recovery semantics are designed.

## Data and Module Boundaries Under Consideration

The likely change surfaces are:

- `agent_resource`: additional optional execution budgets and compatibility rules;
- `agent_store`: typed lifecycle reason, materialized usage and working state;
- `agent_model`: preserve provider-neutral usage and structured control responses;
- `agent_runtime`: context projection, lifecycle interpretation and tool-error policy;
- `agent_runtime::checkpoint`: versioned structured-state recovery;
- CLI inspection: working state, budget consumption and suspension details.

The exact tables and public Rust types remain undecided. Schema design should favor
additive migrations and reading legacy runs without inventing historical state.

## Alternatives Considered

### Continue replaying the complete transcript

This preserves exact provider history and requires the least code. It does not
bound context growth, make working state inspectable or let Context Harness choose
the most useful context. It remains the compatibility implementation behind the
new projection seam, not the intended final long-run strategy.

### Derive all state from the event log on every turn

This keeps one canonical source but makes projection and resume increasingly
expensive and requires historical events to contain every future state detail.
Retaining append-only events plus materialized projections follows the current
run-store pattern and makes inspection efficient.

### Let the model own a free-form scratchpad

This is easy to prompt but difficult to validate, inspect, migrate or budget. A
free-form working summary may be one derived field, but it should not be the only
representation of lifecycle, budgets, artifacts or validation.

### Add Session above Run now

Sessions could group follow-up objectives and user interactions. Current run IDs,
resume and parent/root lineage already cover the known execution needs. Adding a
session now risks ambiguous ownership and lifecycle without a concrete use case.

### Adopt a workflow engine or event-stream service

These systems offer scheduling and orchestration but conflict with the local-first,
single-user scope and duplicate the existing SQLite/event foundation. They remain
non-goals unless product requirements change substantially.

### Enforce monetary cost immediately

This gives users an intuitive budget but requires current provider pricing,
provider-specific billing rules and versioned accounting. Token and call budgets
provide stable first controls; cost can be layered on later.

## Contract Resolution

[ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md) selects separate
lifecycle, outcome, reason, and recovery concepts while preserving the legacy status
projection. [SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md) defines the
allowed states, opt-in control tools, cumulative turn/duration/token/tool budgets,
unknown-usage fallback, inspection fields, legacy migration, and recovery categories.
That contract unblocks the minimum Phase 3 lifecycle implementation.

Structured working-state ownership, selective context projection, recoverable tool
observations, and suspended continuation remain unresolved Phase 5 work. They SHALL
NOT be inferred from the minimum lifecycle contract.

[ADR-0030](../adr/0030-runtime-owned-materialized-working-state.md) and
[SPEC-0027](../spec/0027-structured-agent-working-state.md) now resolve the Phase 5A
boundary: one versioned runtime-owned materialized projection per run, updated
transactionally with canonical events and containing only deterministic facts and
references. Model-authored state and selective context projection remain unresolved
later Phase 5 work and are not implied by that contract.

## Implementation Plan

With the minimum lifecycle contract accepted, the smallest staged path is:

1. **Contract inventory:** map current status, event, checkpoint, CLI JSON and
   migration compatibility surfaces; decide which existing specs require revision.
2. **Projection seam:** introduce `RunContextBuilder` and projection metadata while
   preserving byte-for-byte-equivalent request behavior where practical.
3. **Lifecycle and budgets:** add typed termination reasons plus cumulative token
   and tool-call accounting, with existing statuses mapped compatibly.
4. **Structured working state:** add a versioned materialized representation and
   inspection output, initially populated by deterministic runtime facts.
5. **Context selection:** add bounded recent/history selection and traceable
   summaries or retrieval after evaluation fixtures establish quality criteria.
6. **Recoverable observations:** classify tool failures and add conservative loop
   thresholds without replaying uncertain effects.
7. **Suspension/resume:** support blocked and needs-user-input continuation only
   after lifecycle and checkpoint safety are specified.
8. **Graduation:** add later ADR/spec boundaries for working state, context selection,
   recoverable observations, and suspended continuation before those behaviors ship.

Each stage should be independently reviewable and keep existing runtime behavior
as the compatibility baseline until its replacement has acceptance coverage.

## Acceptance Criteria for the Design Phase

- The current execution and persistence paths are documented accurately.
- Lifecycle, budget, state-authority and projection decisions have named owners or
  explicit resolution criteria.
- A prototype can insert a context-builder seam without changing observable run
  behavior.
- Context-compaction evaluation fixtures include long repository investigation,
  edit/test/fix loops, large tool results and interrupted resume.
- Migration analysis covers existing SQLite rows, checkpoints and CLI JSON.
- ADR and spec boundaries are identified without publishing a decision before the
  open questions are resolved.

## Risks

- A materialized working state can drift from append-only events.
- Summarization can silently remove evidence or introduce unsupported conclusions.
- Provider continuation formats may constrain transcript compaction.
- New lifecycle states can break callers that assume every non-running run is
  terminal or that only four status strings exist.
- Usage accounting may be missing or delayed for some providers.
- Returning tool errors to the model can create retry loops or duplicate effects.
- Extending the delivered declarative tool-binding contract in the same change
  could make this work too broad; interfaces should align, but delivery should
  remain separable.

## Remaining Open Questions

1. **Resolved by ADR-0030/SPEC-0027:** Phase 5A uses one versioned materialized
   snapshot of deterministic facts and canonical-record references per run.
2. **Resolved for Phase 5A by ADR-0030/SPEC-0027:** only the runtime writes the
   snapshot. Model-authored fields require a later constrained contract.
3. What context-compaction algorithm ships first, and how is loss measured?
4. How should OpenAI encrypted reasoning continuation items behave when older
   conversation is summarized or omitted?
5. Which tool errors are safe observations, and what exact repetition key prevents
   loops without blocking legitimate retries?
6. Does user input resume the same run or create a linked run under the existing
   run-lineage model?
7. Does a concrete future workflow require a Session abstraction, or can the
   existing run-lineage model remain sufficient?
8. Does the delivered SPEC-0023 tool-binding identity contain every compatibility
   input that later context projection requires?

## Documentation Graduation

ADR-0028 and SPEC-0025 graduate the minimum lifecycle/budget boundary, and PRD-0014
is Planned. This design remains non-authoritative for the remaining questions above.
Create later ADR/spec contracts before implementing those behaviors, then keep this
document as Planning during implementation and Reference after all planned slices are
delivered.
