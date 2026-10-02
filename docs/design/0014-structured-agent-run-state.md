# DESIGN-0014: Structured Agent Run State and Context Projection

**Status:** Draft
**Date:** 2026-10-01
**Author:** Context Harness contributors
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md), [ADR-0024](../adr/0024-local-agent-runtime.md), [SPEC-0017](../spec/0017-local-agent-execution.md), [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md)

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

This document keeps the architecture deliberately exploratory. It identifies a
small extension seam and the decisions that must be reviewed before an ADR or
authoritative spec is written.

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

A tentative lifecycle separates actively executing, suspended and terminal states:

```text
active:
  running

suspended candidates:
  blocked
  needs_user_input

terminal:
  completed
  failed
  limit_exceeded
  cancelled
```

Whether `blocked` is always suspended is an open question; some blockers may be
terminal for a particular invocation. Status should therefore be paired with a
typed reason and structured detail rather than encoding every distinction in the
status string.

The model needs an explicit way to select completed, blocked or needs-user-input.
Three options remain under consideration:

- structured final output interpreted by the runtime;
- runtime-owned control tools such as `run.finish` and `run.request_user_input`;
- a hybrid that accepts ordinary final responses as backward-compatible completion
  while structured controls express non-completion outcomes.

No option is selected in this draft.

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

## Implementation Plan

No implementation begins from this Draft. After review, the smallest staged path
would be:

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
8. **Graduation:** record selected architectural boundaries in an ADR, update or
   add authoritative specs, then move the PRD to Planned/In Progress.

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

## Open Questions

1. Is working state a single versioned snapshot, normalized records, or a projection
   derived from typed events plus a small materialized summary?
2. Which actor may update plan and summary fields: runtime, model control calls,
   tools, or a constrained combination?
3. Which lifecycle states are terminal, and how does CLI exit status map to
   suspended states?
4. Should explicit completion use structured output, control tools or a hybrid?
5. What context-compaction algorithm ships first, and how is loss measured?
6. How should OpenAI encrypted reasoning continuation items behave when older
   conversation is summarized or omitted?
7. What token-accounting fallback applies when a provider supplies no usage?
8. Which tool errors are safe observations, and what exact repetition key prevents
   loops without blocking legitimate retries?
9. How are legacy runs and checkpoints inspected after schema evolution?
10. Does user input resume the same run, create a linked run, or require a future
    Session abstraction?
11. Which parts belong in revisions to SPEC-0017/0019 versus a new focused spec?
12. Does the delivered SPEC-0023 tool-binding identity contain every compatibility
    input that context projection and checkpoint recovery require?

## Documentation Graduation

This design remains non-authoritative while the questions above are open. Once a
coherent boundary is chosen:

1. create a Proposed ADR recording the structured-state/context-projection choice;
2. draft authoritative behavior in revisions to existing runtime/recovery specs or
   a new focused spec;
3. update PRD-0014 from Draft only after product questions and success measures are
   accepted;
4. keep this document as Planning during implementation and Reference afterward.
