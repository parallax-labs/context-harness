# SPEC-0029: Recoverable Tool Observations and Loop Safeguards

**Status:** Authoritative — implementation pending
**Date:** 2026-10-09
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md),
[DESIGN-0014](../design/0014-structured-agent-run-state.md),
[DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md),
[ADR-0032](../adr/0032-explicit-recoverable-tool-observations.md),
[SPEC-0017](0017-local-agent-execution.md),
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md),
[SPEC-0023](0023-declarative-tool-bindings.md),
[SPEC-0025](0025-durable-run-lifecycle-and-budgets.md), and
[SPEC-0028](0028-bounded-agent-context-projection.md)

## Scope

This specification defines the Phase 5C tool-outcome taxonomy, the trusted public
classification API, model-visible recoverable observation, durable records,
deterministic repetition safeguard and recovery compatibility.

It does not define automatic retries, model-generated summaries, provider retries,
suspended-run continuation, streaming, session semantics, application-specific error
policy or dynamic loading of classification code.

## Definitions

- **Effective arguments:** the validated public arguments combined with immutable
  fixed binding arguments, exactly as covered by approval and execution.
- **No effect:** the failed invocation made no externally observable state change and
  does not leave an operation whose completion is uncertain.
- **Recoverable observation:** sanitized runtime-authored JSON returned to the model
  as the tool result for an explicitly classified no-effect failure.
- **Resolved tool identity:** the public tool name plus binding implementation ID,
  implementation version and resource version when present; compatibility tools use
  their stable built-in implementation identity.
- **Repetition key:** a SHA-256 digest over the canonical JSON tuple defined below.
- **Consecutive:** adjacent completed recoverable outcomes in execution order, with no
  intervening completed tool result or recoverable outcome having another key.

## Public classification API

The main crate SHALL publicly export a typed tool-execution error with these logical
fields:

```text
class: recoverable_error | terminal_error | uncertain_side_effect
code: string
message: string
```

It SHALL implement `std::error::Error` so compiled implementations of the existing
`Tool` trait MAY return it through `anyhow::Error` without changing the trait method
signature. Host registration SHALL remain explicit and compiled; configuration SHALL
NOT load classifiers or executable code.

`code` SHALL match `[a-z][a-z0-9_]{0,63}`. `message` SHALL be valid UTF-8, nonempty
after trimming and at most 4,096 UTF-8 bytes. Constructors SHALL reject invalid
values before execution. Debug and display formatting SHALL expose the class and code
but SHALL NOT expose the message or a wrapped source error.

`recoverable_error` SHALL assert `no effect`. The type SHALL NOT offer a constructor
that combines recoverability with an unknown or possible effect. Trusted tools MUST
provide a newly authored public message; they SHALL NOT copy raw database, network,
provider, MCP, Lua, process or filesystem error text into it.

## Runtime classification

After a tool has passed declaration, argument, capability and approval checks and has
entered `started`, the runtime SHALL walk the returned `anyhow` error chain for the
public typed error.

- `recoverable_error` SHALL follow the recoverable-observation behavior below.
- `terminal_error` SHALL fail the invocation and run with the existing `tool_error`
  run reason, retaining only the public code and sanitized message.
- `uncertain_side_effect` SHALL fail the invocation and run with `tool_error`, mark
  recovery unsafe and retain only the public code and sanitized message.
- An error chain containing no typed error SHALL be `terminal_error` with the existing
  generic `context tool execution failed` diagnostic.

If more than one typed error occurs in an error chain, the outermost typed error SHALL
be authoritative. The runtime SHALL NOT inspect other error strings to refine it.

Denial before start, invalid arguments, missing declarations, approval denial,
cancellation, preparation failure, delegation failure, result serialization failure,
result-size overflow and runtime/storage failure SHALL NOT become recoverable
observations. Existing structured successful results remain successful. In
particular, `process.exec` nonzero exit and captured stderr remain a completed result.

Lua and MCP execution errors SHALL remain terminal in Phase 5C. A future adapter MAY
produce the public typed error only after a separate contract defines a trusted
structured protocol field; it SHALL NOT classify remote text.

## Model-visible observation

For an allowed recoverable failure, the runtime SHALL append one `ModelMessage::Tool`
whose `call_id` is unchanged and whose `content` is compact JSON with exactly this
shape:

```json
{"ok":false,"error":{"type":"recoverable_tool_error","code":"not_found","message":"The requested workspace path does not exist."}}
```

The runtime SHALL author the `ok`, `error` and `type` members and SHALL copy only the
validated public code and message. The observation SHALL count as the tool result for
`ModelRequest::validate`, exchange selection and provider continuation purposes. It
SHALL be subject to the existing tool-result and context byte bounds and SHALL NOT
contain a repetition key, raw arguments, source error, approval data or internal
identity.

The runtime SHALL finish every other tool call already present in the same assistant
response according to existing sequential dispatch rules unless a terminal outcome
or repetition limit stops the run. It SHALL never automatically invoke the failed
tool again; only a later model response may request another call.

## Durable taxonomy and events

The logical terminal invocation outcomes SHALL be:

```text
completed
denied
recoverable_error
terminal_error
uncertain_side_effect
cancelled
```

Persistence MAY retain the legacy `status` column values for migration compatibility,
but SHALL store an additional typed outcome classification for new terminal
invocations. Existing rows with `completed`, `denied` or `failed` and no classification
SHALL preserve their current interpretation; legacy `failed` SHALL be terminal and
never recoverable.

Each terminal tool event SHALL include `call_id` and `outcome`. Recoverable events
SHALL also include the public `code`, sanitized `message`, `repetition_key` and
`consecutive_count`. Terminal and uncertain events MAY include public code/message
when supplied by the typed API. Events, invocation rows, run errors and CLI output
SHALL NOT contain a raw source error.

The exact recoverable observation JSON SHALL be persisted with the invocation result
so inspection and recovery compare bytes derived from canonical data rather than
regenerating prose. Existing history and inspection commands SHALL expose the new
classification through their current event and invocation projections; command names
and paging behavior SHALL NOT change.

## Repetition identity and threshold

Before exposing a recoverable observation, the runtime SHALL compute compact
canonical JSON with recursively lexicographically sorted object keys for:

```json
{
  "version": 1,
  "tool_identity": {
    "name": "workspace.read",
    "implementation_id": "builtin.developer.workspace_read",
    "implementation_version": "1",
    "resource_version": null
  },
  "effective_arguments": {"path":"missing.txt"},
  "error_code": "not_found"
}
```

The repetition key SHALL be the lowercase hexadecimal SHA-256 digest of those bytes.
Secret values SHALL NOT be present in effective arguments; tools whose approval or
effective arguments cannot be persisted safely SHALL NOT emit recoverable errors.
The public message SHALL NOT participate in identity.

The first and second consecutive recoverable outcomes with one key SHALL be persisted
and appended as model-visible observations. On the third consecutive occurrence, the
runtime SHALL persist the recoverable invocation and its `consecutive_count` but SHALL
NOT append the observation or invoke the model again. It SHALL terminate the run as:

```json
{
  "lifecycle": "terminal",
  "outcome": "failed",
  "reason_code": "tool_loop_detected",
  "reason_detail": {
    "repetition_key": "<sha256>",
    "consecutive_count": 3,
    "tool": "workspace.read",
    "error_code": "not_found"
  }
}
```

Reason detail SHALL contain no arguments or message. A completed tool result resets
the consecutive key/count. A recoverable outcome with another key replaces the key
and begins at one. Terminal, denied, uncertain and cancelled outcomes end the run and
therefore require no reset. Counts SHALL be derived or persisted transactionally and
SHALL survive checkpoint resume without decreasing.

Existing maximum turns, duration, total tokens, tool calls, per-turn calls, context
bytes, checkpoint bytes and output bytes remain authoritative outer limits. Phase 5C
SHALL add no automatic backoff, sleep or hidden model/tool attempt.

## Checkpoint and recovery

A checkpoint containing a recoverable observation MAY resume only when the canonical
invocation row is classified `recoverable_error`, contains the exact persisted
observation and precedes the checkpoint. The checkpoint tool message SHALL match that
observation byte-for-byte. Completed invocations retain their existing checks.

Denied, terminal, uncertain, cancelled, requested or started invocations SHALL remain
unsafe for resume. A legacy failed invocation SHALL remain unsafe. Recovery SHALL
restore or derive the last repetition key and consecutive count from canonical
records and SHALL reject a checkpoint that would lower or omit that state.

Bounded context projection MAY later omit a complete recoverable exchange under
SPEC-0028. Schema-v3 recovery MAY accept the omitted invocation only when projection
metadata proves the entire exchange was omitted and canonical records contain the
persisted recoverable observation. It SHALL NOT reconstruct the omitted message or
make the failure eligible for another automatic attempt.

## Static validation and compatibility

Static list, show, validation and task-identity assembly SHALL remain side-effect
free. They SHALL NOT execute classification constructors from configuration, resolve
secrets, open the database, call a provider or tool, access the network, evaluate Lua,
start MCP or spawn a process.

Agents and tools that do not emit the public typed error SHALL preserve current
behavior. The `Tool` trait signature, normal completed result shape, permission and
approval behavior, run history commands, task ownership and complete-transcript or
bounded-context selection SHALL remain compatible.

## Acceptance criteria

Tests SHALL demonstrate:

1. compiled host and built-in tools can explicitly return sanitized recoverable,
   terminal and uncertain typed errors through the unchanged public trait;
2. untyped, Lua and MCP errors remain terminal and raw error values never enter model
   requests, events, invocation rows, run errors or CLI output;
3. denial, invalid arguments, approval denial, cancellation, output overflow and
   uncertain effects never become model-visible observations;
4. recoverable observation JSON is exact, bounded, correlated and followed by a later
   model invocation;
5. changed effective arguments or error codes start a distinct sequence, while a
   third identical consecutive failure terminates before another model call;
6. successful results reset repetition and existing process nonzero-exit behavior
   remains a completed structured result;
7. repetition state and exact observations survive restart, checkpoint validation,
   bounded omission and resume without replaying a tool;
8. legacy invocation rows, agents, CLI JSON, checkpoints and run history preserve
   their existing interpretation; and
9. static inspection performs no network, secret, database, model, tool, Lua, MCP or
   process action.
