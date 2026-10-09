# SPEC-0028: Bounded Agent Context Projection

**Status:** Authoritative — implementation pending
**Date:** 2026-10-09
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md),
[DESIGN-0014](../design/0014-structured-agent-run-state.md),
[DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md),
[ADR-0031](../adr/0031-deterministic-bounded-context-projection.md),
[SPEC-0016](0016-model-runtime.md), [SPEC-0017](0017-local-agent-execution.md),
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md),
[SPEC-0025](0025-durable-run-lifecycle-and-budgets.md), and
[SPEC-0027](0027-structured-agent-working-state.md)

## Scope

This specification defines the first opt-in bounded model-context policy for durable
agent runs, its resource setting, deterministic selection algorithm, working-state
capsule, trace metadata, context-size outcome and checkpoint compatibility.

It does not define model-generated summaries, semantic retrieval over run history,
model-authored working state, observation importance, recoverable tool failures,
repetition safeguards, suspended continuation, provider streaming or a session
abstraction.

## Definitions

- **Source transcript state:** the valid provider-neutral messages still retained for
  projection, plus monotonically assigned logical source indexes and cumulative counts
  for messages omitted by an earlier projection.
- **Stable prefix:** the original system message and objective user message.
- **Exchange:** one assistant message containing one or more tool calls followed
  immediately by exactly one tool-result message for each call, in call order.
- **Working-state capsule:** a runtime-authored system message containing the current
  valid version-1 working-state snapshot and its non-secret row identity.
- **Mandatory context:** stable prefix, working-state capsule, newest exchange, tool
  declarations, output schema and output-token setting.
- **Request bytes:** the length in bytes of compact UTF-8 JSON produced by
  `serde_json::to_vec` for the complete provider-neutral `ModelRequest`.
- **Source range:** an inclusive zero-based logical message-index interval from the
  run's provider transcript. Indexes are never reused after omission.

## Resource configuration

`agent.execution` MAY contain `max_context_bytes` as an integer. When absent, the
runtime SHALL use the existing `compatibility_v1` / `complete_transcript` strategy
without adding a working-state capsule or selecting messages.

When present, `max_context_bytes` SHALL be between 2,097,152 and 8,388,608 bytes,
inclusive. Static agent validation SHALL reject zero, negative, non-integer and
out-of-range values without opening a database, loading working state, constructing
a provider, resolving secrets, invoking a model or tool, accessing the network,
evaluating Lua, starting MCP or spawning a process.

The setting SHALL participate in the parsed resource version and checkpoint binding.
Omitting it SHALL preserve the pre-Phase-5B resource version.

At run creation, the configured value SHALL be copied into the existing persisted
`RunBudgets` JSON as optional `max_context_bytes`. Its absence SHALL serialize as no
new member for compatibility with existing rows and fixtures. Run and CLI inspection
SHALL expose the persisted value through their existing budget projection. Context
projection SHALL enforce the persisted run value and resume SHALL require it to equal
the unchanged resource value; it SHALL NOT be reset or widened during resume.

## Source transcript invariants

Bounded projection SHALL accept only source transcript state whose retained request
satisfies `ModelRequest::validate` after removal of the prior derived capsule and has
the original system message at logical index 0 and objective user message at logical
index 1. Retained messages after the stable prefix SHALL consist exclusively of
complete exchanges. Projection SHALL fail before model invocation if these invariants
do not hold.

An exchange SHALL be indivisible. The runtime SHALL NOT retain an assistant tool call
without every corresponding result, retain a result without its assistant call,
reorder calls or results, rewrite call identifiers, or truncate message content.

The source transcript for a fresh bounded run SHALL initially contain only the stable
prefix. After each successful tool turn, the assistant message and all completed tool
results SHALL be appended before the next projection and assigned consecutive logical
indexes beginning at the prior cumulative source-message count. Once an exchange is
omitted, its content SHALL NOT be reconstructed or reconsidered by a later projection;
its logical indexes remain represented only by cumulative counts and trace ranges.

## Working-state capsule

Before each bounded model request, the runtime SHALL read the run's current supported
working-state projection through the workspace-scoped store API. The state SHALL be
`current`; `stale`, `unavailable` or `unsupported` state SHALL fail the run with the
existing `storage_error` failure reason before provider invocation.

The capsule SHALL be a system message immediately after the original system message
and before the objective user message, so all runtime-authored system context precedes
the user objective. Its content SHALL be compact JSON with exactly these members:

```json
{
  "type": "context_harness.working_state",
  "projection_version": 1,
  "revision": 7,
  "event_cursor": 23,
  "snapshot": {}
}
```

`snapshot` SHALL be the typed version-1 snapshot defined by SPEC-0027. The capsule
SHALL NOT include row timestamps, inspection status, objective text, raw event
payloads, model messages, tool arguments/results, provider errors, claim tokens,
credential values or any field forbidden by SPEC-0027.

The capsule is derived context, not a canonical model message. A later projection
SHALL replace the prior capsule rather than adding another one. It SHALL NOT be
written into the source transcript, working-state snapshot, run events or tool
records as raw content.

## `bounded_suffix_v1` selection

For a configured run, the builder SHALL identify every complete exchange after the
stable prefix and SHALL select them as follows:

1. The newest exchange, when one exists, is mandatory.
2. Starting with the next-newest exchange, the builder SHALL add whole exchanges
   while the complete request remains at or below `max_context_bytes`.
3. The builder SHALL stop at the first older exchange that would exceed the bound;
   it SHALL NOT skip that exchange to search for smaller, still-older exchanges.
4. Selected exchanges SHALL appear in original chronological order after the capsule.
5. Exchanges not selected SHALL be omitted in full.

The tools, output schema and maximum-output-token setting SHALL equal the source
request values. Projection SHALL validate the selected request and compute its exact
request bytes before provider invocation.

If mandatory context exceeds `max_context_bytes`, the runtime SHALL NOT truncate any
content or call the provider. It SHALL finish the run as `limit_exceeded` with reason
code `context_bytes`. `LimitReason` SHALL add the serialized value `context_bytes`;
legacy status SHALL continue to project this outcome as `failed` under SPEC-0025.

No request emitted by `bounded_suffix_v1` SHALL exceed the configured bound.

## Opaque provider continuations

An assistant message's `ProviderContinuation` SHALL remain opaque and SHALL follow
the exchange containing that message. If the exchange is selected, the continuation
SHALL be retained byte-equivalently. If the exchange is omitted, the continuation
SHALL be omitted.

Projection metadata, events, diagnostics and the working-state capsule SHALL NOT
contain continuation contents, hashes, byte samples or provider item types. The
runtime SHALL NOT detach a continuation from its assistant message or attach it to a
different exchange.

OpenAI fixture tests SHALL prove that a selected suffix containing encrypted
reasoning continuation items maps to the existing stateless Responses request
without logging those items. Ollama and providers without continuation data SHALL
use the same exchange-selection algorithm.

## Projection metadata and events

Every bounded projection SHALL produce typed metadata with exactly these fields:

```json
{
  "builder": "bounded_v1",
  "strategy": "bounded_suffix_v1",
  "max_context_bytes": 2097152,
  "request_bytes": 12034,
  "source_message_count": 14,
  "included_message_count": 8,
  "omitted_message_count": 6,
  "included_exchange_count": 2,
  "omitted_exchange_count": 1,
  "retained_source_ranges": [{"start": 0, "end": 1}, {"start": 8, "end": 13}],
  "working_state_projection_version": 1,
  "working_state_revision": 7,
  "working_state_event_cursor": 23
}
```

`source_message_count` SHALL be the cumulative number of logical source messages
through the boundary. `included_message_count + omitted_message_count` SHALL equal
that value, including omissions made at earlier boundaries. The included and omitted
exchange counts SHALL likewise sum to the cumulative number of completed exchanges.
Counts and ranges SHALL
describe logical source transcript messages only; the derived capsule is not a source
message. The stable prefix range SHALL always be present. Ranges SHALL be ordered,
non-overlapping and inclusive and SHALL identify every retained source message.
Metadata SHALL NOT contain message content, tool arguments/results, continuation data,
hashes of omitted content, credentials or raw provider errors.

The runtime SHALL append a `context.projected` event before every bounded model
request. Its payload SHALL contain only the projection metadata. The event and the
checkpoint for that model boundary SHALL be durable before model invocation. The
projection metadata's working-state cursor MAY precede the event and checkpoint
events that record the boundary; this relationship is expected and SHALL be
inspectable.

Complete-transcript runs SHALL preserve their existing `context.resolved` event and
metadata behavior. Phase 5B SHALL NOT add per-turn `context.projected` events to
unconfigured compatibility runs.

## Checkpoint schema version 3

New bounded runs SHALL use checkpoint schema version 3. The checkpoint SHALL persist:

- the selected `ModelRequest`, including retained opaque continuations;
- the projection metadata above;
- the configured `max_context_bytes`;
- the logical source-index cursor and message/exchange omission counts accumulated
  through that boundary;
- the existing workspace, run, agent, provider implementation, tool binding, policy,
  budget, usage, deadline and outcome-schema compatibility fields.

The selected request SHALL remain within both `max_context_bytes` and the existing
16 MiB checkpoint limit. The checkpoint binding SHALL include the context builder,
strategy version and configured byte bound.

Resume SHALL continue to accept schema version 1 and 2 checkpoints exactly as defined
by SPEC-0019 and SPEC-0025. Those checkpoints SHALL resume with complete-transcript
projection when the unchanged resource omits `max_context_bytes`.

A schema version 3 bounded checkpoint SHALL resume from its selected request and
projection state. Resume SHALL validate the stable prefix, exchange integrity,
resource byte bound, projection metadata, tools, completed tool results and all
existing recovery bindings before reopening the run. Cumulative logical message and
exchange counts SHALL equal the complete pre-checkpoint `model.responded`
`tool_call_count` metadata and canonical invocation records. Calls retained in the
selected request SHALL match their canonical names, arguments and serialized results.
A completed canonical invocation MAY be absent from the selected request only when
the projection ranges and cumulative counts classify its complete exchange as
omitted. Pending, failed, denied or uncertain invocations SHALL retain the existing
fail-closed recovery behavior. Resume SHALL NOT reconstruct omitted messages from
events, tool records or older checkpoints, and SHALL NOT restore an older request over
a newer safe checkpoint.

After resume, newly completed exchanges SHALL be appended to the restored selected
request and `bounded_suffix_v1` SHALL run again. Previously omitted message and
exchange counts SHALL remain cumulative even though their contents are absent.

## Compatibility and inspection

Direct runs and worker-owned runs SHALL use the same selected strategy from their
agent resource. Provider and tool interfaces, tool authorization, tool execution,
working-state persistence, task scheduling and run history schemas SHALL otherwise
remain unchanged.

`ctx agent inspect` SHALL expose `context.projected` events through its existing
bounded event paging. It SHALL NOT duplicate raw projected requests or continuation
data into top-level run JSON. Existing history output SHALL remain unchanged.

Static resource inspection SHALL report the configured byte bound without reading
run state. Runtime projection SHALL use only the existing database and in-memory
request; it SHALL NOT access the network, invoke a model or tool, evaluate Lua, start
MCP or spawn a process. The later provider call is outside projection.

## Acceptance criteria

- Resource fixtures prove omission preserves prior versions and explicit bounds are
  validated offline and participate in identity.
- Selection fixtures prove exact byte bounds, stable-prefix/capsule placement,
  newest-first whole-exchange selection, chronological output and deterministic
  retained ranges.
- Boundary fixtures prove exact-fit succeeds, one-byte overflow compacts, and
  oversized mandatory context yields `limit_exceeded/context_bytes` without a model
  call.
- Long-run fixtures exceed 25 model/tool turns while every configured request remains
  bounded and the objective, latest observation and artifact references remain
  present.
- Continuation fixtures prove selected OpenAI encrypted items survive byte-equivalently
  and omitted items never appear in metadata, events or diagnostics.
- Checkpoint fixtures prove version 3 resume continues from bounded state, omission
  counts remain cumulative, version 1 and 2 recovery remains compatible and tampered
  projection metadata is rejected before mutation.
- Inspection fixtures prove per-turn metadata is paged and contains no raw messages,
  tool payloads, continuation data, secrets or provider errors.
- Existing unconfigured runtime, provider, tool, delegation, task, checkpoint,
  resume and history fixtures retain complete-transcript behavior.
