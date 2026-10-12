# SPEC-0030: Suspended Run Continuation

**Status:** Authoritative — implemented
**Date:** 2026-10-12
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md),
[DESIGN-0014](../design/0014-structured-agent-run-state.md),
[DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md),
[ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md),
[ADR-0029](../adr/0029-durable-agent-task-ownership.md),
[ADR-0031](../adr/0031-deterministic-bounded-context-projection.md),
[ADR-0033](../adr/0033-same-run-suspended-continuation.md),
[SPEC-0016](0016-model-runtime.md), [SPEC-0019](0019-checkpoints-recovery-and-artifacts.md),
[SPEC-0025](0025-durable-run-lifecycle-and-budgets.md),
[SPEC-0026](0026-durable-agent-tasks.md), and
[SPEC-0028](0028-bounded-agent-context-projection.md)

## Scope

This specification defines explicit continuation of standalone durable runs stopped
as `blocked` or `needs_user_input`. It governs public host and CLI entry points,
conversation projection, persistence, duration accounting, concurrency, recovery and
compatibility.

It does not define automatic worker continuation, polling or retry, delegated-tree
recovery, MCP-backed recovery, a session abstraction, provider streaming,
application-specific input policy or replacement runs.

## Definitions

- **Suspension:** the committed `suspended/blocked` or
  `suspended/needs_user_input` run state and its typed reason.
- **Suspension exchange:** the exact assistant control-call message that caused a
  suspension plus the runtime-authored result that resolves it during continuation.
- **Continuation input:** a trusted host-supplied answer or environment update used
  to resolve one suspension.
- **Suspension sequence:** the stopped run event sequence that uniquely identifies
  the suspension being continued.
- **Suspended interval:** time from the suspension's committed `completed_at` through
  the successful continuation transition timestamp.
- **Execution elapsed:** wall-clock age minus the sum of successfully continued
  suspended intervals. This is the value charged to the duration budget.

## Supported entry points

`AgentRuntime` and `AgentHost` SHALL expose an additive `continue_run` operation with
logical arguments:

```text
run_id: string
input: string
cancellation: receiver
```

The CLI SHALL expose:

```text
ctx agent continue <run-id> <input> [--json] [--non-interactive]
```

`input` SHALL be nonempty after trimming and no larger than 8 KiB of UTF-8. The
runtime SHALL preserve the supplied bytes after validation; it SHALL NOT trim,
interpret, interpolate or execute them. CLI JSON and human output SHALL use the same
completed-run and non-success exit behavior as `ctx agent run` and `ctx agent resume`.

The operation SHALL apply only to a run whose canonical lifecycle is `suspended` and
whose outcome is `blocked` or `needs_user_input`. Ordinary `resume` SHALL continue to
reject suspended runs. `continue_run` SHALL reject active, terminal, completed,
failed, cancelled and limit-exceeded runs without mutation.

## Conversation boundary

When a model returns a valid `run.blocked` or `run.request_user_input` control call,
the runtime SHALL persist the exact neutral `ModelMessage::Assistant`, including its
single control call and any opaque provider continuation, as pending suspension
state bound to the suspension sequence. The existing typed reason remains the public
description of the suspension. Incidental assistant text SHALL not replace or expand
that reason.

Pending suspension state SHALL be bounded by the existing 16 MiB checkpoint limit.
An oversized state SHALL stop as `limit_exceeded/checkpoint_size` rather than create
a suspension that cannot later continue. Events, reason detail and diagnostics SHALL
not copy opaque continuation items.

On continuation, the runtime SHALL append the pending assistant message followed by
one `ModelMessage::Tool` with the same control-call ID. Its compact JSON content SHALL
have exactly this shape:

```json
{"type":"run_continuation","outcome":"needs_user_input","reason_code":"information_required","prompt":"Which branch should I use?","input":"Use main."}
```

For a blocked run, `outcome` SHALL be `blocked`, `prompt` SHALL contain the persisted
blocked `message`, and `input` SHALL contain the trusted environment update. For a
needs-input run, `outcome` SHALL be `needs_user_input`, `prompt` SHALL contain the
persisted `question`, and `input` SHALL contain the trusted answer. `reason_code`
SHALL equal the persisted typed code. JSON escaping SHALL be canonical and no other
members SHALL be present.

The runtime-authored result resolves the control call for model-protocol purposes but
SHALL NOT create a `tool_invocations` row, execute a tool, require approval, consume
the ordinary tool-call budget or count toward the recoverable-tool repetition rule.
The complete exchange SHALL satisfy `ModelRequest::validate` before persistence.
Opaque provider continuation SHALL remain byte-equivalent and attached to its
assistant message under SPEC-0016 and SPEC-0028.

The suspension exchange SHALL enter the complete source transcript. Bounded context
projection SHALL treat it as one indivisible assistant/tool exchange and MAY later
omit it only under the existing suffix and working-state rules. Continuation SHALL
not introduce a new context category or a model-generated summary.

## Validation and safety preconditions

Before mutation, continuation SHALL:

1. resolve the run within the current canonical workspace;
2. reject MCP-backed resources and any run that is a delegated child or has
   descendants;
3. acquire the same exclusive per-run execution lock used by resume;
4. re-read the run and suspension after locking;
5. validate unchanged agent name and version, model alias and provider/model
   identity, declared tools and implementations, host policy, resource budgets,
   checkpoint schema and binding identity;
6. verify that the pending control call matches the persisted outcome, reason code
   and prompt and was declared by the resource;
7. verify that all ordinary effects before suspension are captured by the latest
   completed checkpoint and that no incomplete, denied, terminal or uncertain
   invocation crosses the recovery boundary; and
8. verify all non-duration cumulative budgets still have remaining capacity.

These checks SHALL be fail-closed and SHALL use the current SPEC-0019 recovery
identity rules. A failed check SHALL leave lifecycle, deadline, suspension state,
checkpoint and events unchanged. Continuation SHALL make no provider or tool call
until the durable transition described below commits.

## Atomic continuation transition

After validation, one SQLite transaction SHALL:

1. conditionally match the run ID, workspace ID, suspension lifecycle, outcome,
   suspension sequence and expected last event sequence;
2. record the continuation input and its suspension sequence in canonical
   continuation state;
3. add the suspended interval to cumulative suspended duration using checked
   arithmetic;
4. save a new checkpoint containing the resolved suspension exchange and unchanged
   cumulative budgets and usage;
5. change the same run to `active` with no current outcome or reason;
6. clear compatibility output/error/completion fields as ordinary resume does; and
7. append `run.continued` with the prior outcome, reason code, suspension sequence,
   continuation byte length, cumulative suspended duration, budgets and usage.

The event SHALL NOT contain continuation input, prompt content, opaque provider data
or credential values. The continuation state and checkpoint MAY contain the input
because it is required conversation state and SHALL follow existing local run-data
access and retention rules.

The transaction SHALL occur before another provider request. A crash after commit
SHALL therefore recover from the new completed checkpoint without re-appending the
input. A crash before commit SHALL leave the original suspension intact. A stale,
duplicate or racing request SHALL fail its conditional transition and return or
report the already persisted current run without creating another continuation.

Each later suspension SHALL have a new suspension sequence and MAY be continued once.
Continuation SHALL never change the run ID, task `run_id`, lineage or original input.

## Duration and other budgets

Model turns, input/output/total tokens, responses with or without usage and ordinary
tool calls SHALL remain cumulative across every continuation and SHALL never reset or
decrease.

The duration budget SHALL charge execution elapsed. For a run with no successful
suspended continuation, execution elapsed and the existing wall-clock elapsed value
are identical. At continuation, the runtime SHALL extend the effective absolute
deadline by exactly the newly committed suspended interval. Multiple continued
suspensions SHALL accumulate. Waiting after a crash, terminal failure or
cancellation SHALL not become excluded time.

The existing public `elapsed_ms` compatibility field SHALL remain wall-clock age.
Inspection SHALL additionally expose cumulative `suspended_ms` and derived
`execution_elapsed_ms`. Checkpoints SHALL persist the cumulative suspended duration
and effective deadline, and recovery SHALL reject values inconsistent with the
canonical run record.

If execution elapsed has already exhausted `timeout_seconds`, continuation SHALL
reject without mutation. Once continued, duration preflight and enforcement SHALL use
the extended effective deadline. Continuation SHALL not grant a new timeout or alter
any configured budget.

## Tasks, cancellation and recovery

A durable task linked to the run SHALL retain its terminal scheduling state and
immutable `run_id`. Explicit continuation operates on the run through a configured
host; it SHALL not requeue, reclaim or lease the task. Workers and reconcilers SHALL
not infer continuation input or invoke `continue_run` automatically.

After the atomic transition, ordinary checkpoint recovery MAY resume the active run
under SPEC-0019. If the run suspends again, its recovery disposition SHALL again be
`suspended`. A suspended run with missing, malformed or incompatible pending
suspension state SHALL remain inspectable but SHALL reject continuation as
`restart_required`; it SHALL not fabricate a conversation exchange.

Cancellation observed after continuation commits and before another stopped outcome
SHALL follow the existing active-run cancellation contract. Cancellation cannot win
the already completed conditional transition and then remove its durable input.

## Static inspection and compatibility

Static resource list, show and validation SHALL recognize no new configurable code
or authority. They SHALL remain free of database access, secret resolution, provider
or tool calls, network requests, Lua evaluation, MCP startup and process startup.

Existing agents, runs, checkpoints, suspension reason shapes, ordinary crash resume,
CLI run/history output, task scheduling, OpenAI/Ollama/fake provider behavior and
tool accounting SHALL remain compatible. The persistence migration SHALL be additive
and SHALL not invent pending suspension messages for historical suspended runs.
Historical suspensions without the new state remain inspectable and cannot continue.

Continuation input and prompt content SHALL not be printed by default human
inspection or copied into event summaries. JSON checkpoint or canonical-state access
retains its existing trusted local-data boundary. Raw provider errors and credential
values SHALL never enter continuation state.

## Acceptance criteria

Tests SHALL demonstrate:

1. standalone blocked and needs-input runs continue under the same run ID through
   public Rust and CLI entry points;
2. the exact assistant control call, opaque provider continuation and canonical
   runtime result form a valid exchange before the next model invocation;
3. continuation creates no ordinary invocation, approval or tool-call debit;
4. model, token and tool usage remain cumulative, while only committed suspended
   intervals extend the duration deadline;
5. repeated suspensions accumulate duration exclusions without resetting budgets;
6. stale, duplicate and racing requests append the continuation input at most once;
7. crashes immediately before and after the atomic transition either preserve the
   suspension or recover the committed continuation without duplicate model input;
8. resource, provider/model, tool, policy, budget, checkpoint and effect drift reject
   without mutation or external calls;
9. delegated, MCP-backed, historical and malformed suspensions reject conservatively;
10. linked tasks retain their terminal scheduling record and workers never continue
    them automatically;
11. legacy runs, resume, history, checkpoints, providers and agents retain their
    behavior; and
12. static inspection performs no network, secret, database, model, tool, Lua, MCP
    or process action.
