# ADR-0031: Deterministic Bounded Context Projection

**Status:** Accepted
**Date:** 2026-10-09

## Context

Context Harness currently sends the complete valid provider transcript on every
model turn. The compatibility `RunContextBuilder` isolates that behavior and records
projection metadata, while checkpoints persist the complete `ModelRequest`. This is
safe for short runs but makes request and checkpoint size grow with every tool turn.

PRD-0014 requires long-running runs to retain their objective, active state, recent
observations and artifact references without consuming every prior message verbatim.
Phase 5A established a bounded, runtime-owned working-state projection, but did not
send it to providers or select transcript content. DESIGN-0014 left the initial
selection algorithm and treatment of opaque provider continuations unresolved.

The first bounded policy must be provider-neutral, deterministic, inspectable and
safe to resume. It must not require another model call, invent a summary, expose
encrypted reasoning, split tool-call/result relationships or silently change
existing agents.

## Decision

The first bounded strategy uses explicit, byte-budgeted deterministic suffix
selection. Agent resources opt in with `agent.execution.max_context_bytes`; resources
that omit it continue to use the complete-transcript compatibility strategy.

Every bounded request retains the system prompt, objective, current version-1
working-state capsule, tool declarations and newest complete conversation exchange.
An assistant message containing tool calls and all corresponding tool-result messages
form one indivisible exchange. Additional complete exchanges are retained newest
first while the provider-neutral serialized request remains within the configured
byte budget. Selected exchanges are emitted in their original order.

The runtime does not summarize omitted messages in Phase 5B. It records non-secret
selection metadata, including source, retained and omitted counts, retained source
ranges, request bytes, strategy version and working-state revision/cursor. The
metadata is persisted with a versioned checkpoint and exposed through ordinary run
events.

Opaque provider continuation data belongs to its assistant exchange. Retaining that
exchange retains the continuation unchanged; omitting the exchange omits the
continuation. The runtime never extracts, logs, hashes, summarizes or independently
replays encrypted reasoning items.

The newest complete exchange is mandatory because it contains the observation that
caused the next model turn. If the stable context and that exchange cannot fit, the
run stops with a typed context-size limit instead of truncating message content or
issuing an oversized request.

New checkpoints use schema version 3 and persist the already-projected request plus
its projection state. Resume accepts existing version 1 and 2 checkpoints under
their existing complete-transcript rules. It never reconstructs omitted provider
messages from events or restores an older, larger transcript over the bounded
checkpoint.

## Alternatives Considered

### Model-generated rolling summaries

A summary can preserve semantic detail more efficiently than suffix selection, but
it adds another model invocation, can introduce unsupported claims, requires source
provenance and creates new budget, failure and recovery semantics. It remains a later
derived-state option.

### Token-based limits

Provider tokenizers are not available uniformly, and provider-reported input usage
arrives only after a request. UTF-8 JSON byte measurement is deterministic and
provider-neutral. It is an application bound, not a promise about a provider context
window.

### Truncate individual messages or tool results

This would preserve more exchanges but could corrupt JSON results, hide decisive
evidence or make a tool call appear to have a different result. Phase 5B keeps
messages intact and fails closed when mandatory content cannot fit.

### Always enable a fixed bound

Changing every existing agent would alter request behavior and resource/checkpoint
compatibility without operator intent. Explicit opt-in preserves the current
contract while making bounded behavior testable and deployable.

### Retrieve old transcript fragments semantically

Retrieval may improve relevance, but it requires an indexing, scoring, provenance
and freshness contract for run observations. It is independent of the first bounded
selection policy and remains deferred.

## Consequences

- Configured runs have a deterministic upper bound on serialized model requests.
- Existing agents remain on complete transcript projection until explicitly opted in.
- Older useful details can be omitted because Phase 5B does not claim semantic
  summarization or relevance ranking.
- The working-state capsule preserves current runtime facts and artifact references,
  but not a model-authored plan or historical narrative.
- Checkpoint schema and recovery validation must evolve while retaining version 1
  and 2 compatibility.
- Providers receive only complete assistant/tool exchange units; opaque continuation
  data never crosses exchange boundaries.
- Later summary or retrieval strategies require a new versioned contract rather than
  silently changing `bounded_suffix_v1`.

## References

- [PRD-0014](../prd/0014-durable-agent-runs.md)
- [DESIGN-0014](../design/0014-structured-agent-run-state.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [ADR-0030](0030-runtime-owned-materialized-working-state.md)
- [SPEC-0027](../spec/0027-structured-agent-working-state.md)
- [SPEC-0028](../spec/0028-bounded-agent-context-projection.md)
