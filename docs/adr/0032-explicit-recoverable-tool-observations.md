# ADR-0032: Explicit Recoverable Tool Observations

**Status:** Accepted
**Date:** 2026-10-09

## Context

The local agent runtime currently treats every error returned by `Tool::execute` as
fatal. That default prevents accidental retries after denied, partially applied or
otherwise uncertain operations, but it also terminates useful read/diagnostic loops
for safe conditions that a model could correct, such as a trusted tool reporting that
an input was not found.

PRD-0014 requires a typed outcome taxonomy and bounded retry policy. DESIGN-0014
left two questions open: which failures are safe to show to the model, and what
identity distinguishes a legitimate changed attempt from an identical loop. The
answer must work for built-in and compiled host tools without parsing arbitrary error
strings, exposing raw implementation errors or weakening approval and capability
checks. Existing tools and agents must keep their fail-closed behavior until they
explicitly adopt the new contract.

## Decision

Recoverability is an explicit assertion by trusted tool implementation code. Context
Harness adds a public typed tool-execution error carrying a stable public code, a
bounded model-safe message and an effect classification. A tool opts into a
recoverable observation only by returning the typed `recoverable_error` variant and
asserting that the failed attempt produced no external effect. The existing
`Tool::execute` signature remains unchanged; the runtime recognizes the public type
through the `anyhow` error chain. Untyped errors remain terminal.

The runtime never infers recoverability from text, an OS error kind, a tool name,
capabilities, an MCP response, a Lua exception or a provider response. Permission and
approval denial, invalid model arguments, cancellation, output-bound violations,
preparation failures, delegation failures and any known or possible partial side
effect remain non-recoverable. Normal structured tool results, including nonzero
`process.exec` exit codes, remain successful observations rather than errors.

A recoverable error becomes a normal provider-neutral tool-result message with a
small runtime-authored JSON envelope. The durable invocation remains distinguishable
from success and records only the public code/message and classification, never the
raw source error. Checkpoint recovery may treat such an invocation as captured only
when the exact observation is present in the checkpoint transcript.

Identical recoverable loops use a runtime-computed key over the immutable resolved
tool identity, canonical effective arguments and public error code. Human-readable
messages are excluded. Two consecutive occurrences of one key may reach the model;
the third terminates the run with `tool_loop_detected` before another model call.
A completed tool result or a recoverable failure with a different key starts a new
consecutive sequence. Existing turn, duration and tool-call budgets remain the outer
bounds.

## Alternatives Considered

### Infer safe errors from `anyhow` messages or operating-system kinds

Strings and source chains are implementation details, can contain secrets or indexed
content and do not prove whether a side effect occurred. Classification by inference
would make safety depend on unstable wording.

### Make every read-only tool error recoverable

Capability metadata describes authority, not execution state. A read-only transport
failure may still have uncertain remote semantics, and an implementation bug should
not become model input automatically.

### Change `Tool::execute` to a new result type

That would break every compiled host tool. Recognizing an additive public error type
through `anyhow` preserves the existing trait and defaults.

### Count every failure globally

Distinct diagnostic attempts are often legitimate. A key containing stable tool
identity, canonical effective arguments and error code stops exact repetition while
allowing the model to change its approach.

### Let tools choose their own retry threshold

Per-tool thresholds complicate static identity, configuration, inspection and
recovery. A single versioned runtime rule is deterministic and can be revised through
a later contract if evaluation evidence requires it.

## Consequences

- Existing tools remain fail-closed unless trusted code explicitly adopts the typed
  recoverable variant.
- Models can repair a narrow class of no-effect failures without losing the run.
- Public error messages become part of model context and therefore must be sanitized
  and bounded by the tool implementation and runtime.
- Exact repeated failures stop deterministically without comparing unstable prose.
- Persistence and checkpoint validation must distinguish recoverable observations
  from terminal or uncertain failures while retaining legacy rows.
- Lua and MCP failures remain terminal in the first slice; future adapters require a
  separately trusted structured mapping rather than parsing remote errors.

## References

- [PRD-0014](../prd/0014-durable-agent-runs.md)
- [DESIGN-0014](../design/0014-structured-agent-run-state.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [ADR-0025](0025-declarative-tool-bindings.md)
- [ADR-0028](0028-typed-run-outcomes-and-compatibility.md)
- [SPEC-0029](../spec/0029-recoverable-tool-observations.md)
