# SPEC-0025: Durable Run Lifecycle and Cumulative Budgets

**Status:** Authoritative — implementation pending
**Date:** 2026-10-03
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md),
[DESIGN-0014](../design/0014-structured-agent-run-state.md),
[ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md),
[SPEC-0017](0017-local-agent-execution.md),
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md),
[SPEC-0021](0021-agent-delegation.md)

## Scope

This specification defines canonical local-agent lifecycle state, typed outcomes and
reasons, the compatibility projection for existing run records, explicit model control
calls, cumulative execution budgets, inspection, and recovery categories. It governs
direct and delegated local runs and the run outcome consumed by future background
tasks.

It does not define background-task scheduling, context compaction, structured working
state, recoverable tool errors, session semantics, or continuation of suspended
conversations. Those capabilities require separate contracts.

## Definitions

- **Lifecycle state:** whether a run is `active`, `suspended`, or `terminal`.
- **Outcome:** the typed meaning of a stopped run. Active runs have no outcome.
- **Reason:** a non-secret code and bounded structured detail explaining an outcome.
- **Legacy status:** the existing `running|completed|failed|cancelled` compatibility
  field retained for old readers.
- **Usage:** cumulative model attempts, elapsed duration, reported tokens, and started
  tool calls for the complete run, including resumed execution.
- **Budget:** a configured upper bound on one usage dimension.
- **Recovery disposition:** the current safety classification used by hosts and future
  workers; it is not itself a lifecycle state or outcome.

## Canonical state and outcomes

Every new run SHALL persist one of these valid combinations:

| Lifecycle | Outcome | Meaning |
|---|---|---|
| `active` | absent | execution may currently proceed |
| `suspended` | `blocked` | an external condition prevents progress |
| `suspended` | `needs_user_input` | a concrete user decision or information is required |
| `terminal` | `completed` | the objective completed with final output |
| `terminal` | `failed` | execution stopped because of an error or invalid behavior |
| `terminal` | `limit_exceeded` | an enforced cumulative limit stopped execution |
| `terminal` | `cancelled` | a host or user cancellation stopped execution |

No other combination is valid. A stopped run SHALL have exactly one outcome and one
reason code. The structured reason detail SHALL be a JSON object no larger than 8 KiB
when serialized. It SHALL NOT contain credential values, raw provider error bodies,
raw tool exception text, or unrestricted model/tool payloads.

The initial reason-code vocabulary SHALL be:

| Outcome | Allowed reason codes |
|---|---|
| `completed` | `final_response`, `legacy_completed` |
| `blocked` | `external_dependency`, `environment_unavailable`, `policy_restriction`, `resource_unavailable` |
| `needs_user_input` | `decision_required`, `information_required` |
| `failed` | `model_error`, `model_refusal`, `model_output_truncated`, `content_filtered`, `invalid_model_response`, `tool_error`, `permission_denied`, `storage_error`, `accounting_error`, `recovery_rejected`, `legacy_failure` |
| `limit_exceeded` | `model_turns`, `duration`, `total_tokens`, `token_usage_unavailable`, `tool_calls`, `tool_calls_per_turn`, `checkpoint_size`, `output_size` |
| `cancelled` | `cancellation_requested`, `ancestor_cancelled`, `legacy_cancelled` |

Adding a reason code is a public contract change. Unknown codes SHALL be preserved by
inspection but treated conservatively as non-success and `restart_required` by code
that does not understand them.

## Legacy compatibility projection

`agent_runs.status` SHALL remain present and SHALL contain only its existing four
values. Canonical state SHALL project to it as follows:

| Canonical state/outcome | Legacy status |
|---|---|
| active | `running` |
| terminal/completed | `completed` |
| terminal/cancelled | `cancelled` |
| suspended/blocked | `failed` |
| suspended/needs_user_input | `failed` |
| terminal/failed | `failed` |
| terminal/limit_exceeded | `failed` |

New library and runtime code SHALL branch on typed canonical fields, not the legacy
projection. Generic writes SHALL NOT accept arbitrary lifecycle, outcome, reason, or
legacy status strings.

An additive migration SHALL backfill historical rows deterministically:

- `running` becomes active with no outcome;
- `completed` becomes terminal/completed with `legacy_completed`;
- `failed` becomes terminal/failed with `legacy_failure`; the existing error remains
  available and SHALL NOT be rewritten; and
- `cancelled` becomes terminal/cancelled with `legacy_cancelled`.

Migration SHALL NOT infer blocked, needs-input, limit, or detailed failure meaning
from historical prose. Historical event payloads and checkpoints SHALL remain
unchanged.

## Completion and runtime control calls

A normal provider response with `FinishReason::Completed` SHALL continue to produce
terminal/completed with reason `final_response`. Existing agents require no resource
change and receive no new tool declarations implicitly.

Two reserved runtime control tools SHALL be available to agent resources:

```text
run.blocked
run.request_user_input
```

An agent MUST list a control tool in `agent.tools` before it is advertised or
accepted. These names SHALL be owned by the runtime and SHALL NOT be registered or
overridden by compiled, Lua, MCP, or other host tool catalogs. They confer no
capability and require no approval because they perform no external side effect.
Static resource validation SHALL recognize them without opening the database,
constructing a provider, starting a process, or accessing the network.

`run.blocked` SHALL accept exactly:

```json
{
  "code": "external_dependency | environment_unavailable | policy_restriction | resource_unavailable",
  "message": "nonempty UTF-8 text"
}
```

`run.request_user_input` SHALL accept exactly:

```json
{"code":"decision_required|information_required","question":"nonempty UTF-8 text"}
```

`message` and `question` SHALL each be at most 8 KiB. Unknown fields, blank values,
invalid codes, multiple control calls in one response, or a control call combined
with an ordinary tool call SHALL produce terminal/failed with
`invalid_model_response`. A valid control call SHALL append the matching run event and
set the suspended outcome atomically. It SHALL NOT execute an ordinary tool, create a
tool invocation, or require another model turn. Incidental response text SHALL NOT
override or expand the structured control detail.

Suspended continuation is not implemented by this contract. `ctx agent resume` SHALL
reject suspended outcomes without mutation until a later specification defines how
user or environment input becomes valid provider conversation. Hosts and workers
SHALL retain and report the suspension instead of converting it to failure or
starting a replacement run automatically.

## Cumulative budgets and usage

Execution limits SHALL retain existing `max_turns` and `timeout_seconds` behavior and
add optional positive `max_total_tokens` and `max_tool_calls` fields. Omission means
that dimension is not budget-limited. Static validation SHALL reject zero, overflow,
and unknown execution fields. The fixed maximum of 32 tool calls in one model
response remains independently enforced.

The runtime SHALL persist these cumulative counters:

- `model_turns`: every durable `model.requested` attempt, including failed,
  interrupted, and resumed attempts;
- `input_tokens`, `output_tokens`, and `total_tokens`: checked sums of each provider
  response that reports usage;
- `responses_with_usage` and `responses_without_usage`;
- `tool_calls`: invocations that reached `started`; and
- elapsed duration, derived from original `created_at` and the current or stopped
  timestamp rather than accumulated process uptime.

Token accounting state SHALL be `complete` when every recorded response reports
usage, `partial` when some but not all do, and `unavailable` when none do. Missing
usage SHALL never be stored or displayed as zero.

Budgets SHALL be checked before starting work when the relevant count is already
exhausted and again immediately after recording provider usage. A response that makes
reported tokens exceed `max_total_tokens` SHALL produce terminal/limit_exceeded with
`total_tokens`; its tool calls and final text SHALL NOT be executed or accepted as a
completed outcome. The provider call itself cannot be undone.

If `max_total_tokens` is configured and any response omits usage, that response SHALL
be durably recorded and the run SHALL stop terminal/limit_exceeded with
`token_usage_unavailable`. The runtime SHALL NOT execute its tools or accept it as a
completed outcome. Without a token budget, missing usage only changes accounting
state and SHALL NOT stop execution.

Before persisting or executing any ordinary calls from one response, the runtime
SHALL verify that the complete response would fit both the cumulative
`max_tool_calls` remainder and the fixed per-turn limit. If either check fails, none
of that response's ordinary calls SHALL be requested or executed, and the run SHALL
stop with `tool_calls` or `tool_calls_per_turn` respectively. Valid runtime control
calls do not create tool invocations and do not consume the ordinary tool-call
counter.

Exhausting `max_turns` SHALL produce limit_exceeded/model_turns. Expiring the original
wall-clock deadline SHALL produce limit_exceeded/duration. Checkpoint and final-output
bounds SHALL map to their corresponding limit reasons. Provider refusal, content
filtering, truncated provider output, and ordinary model/tool errors remain failed
outcomes rather than budget exhaustion.

Cancellation observed before another terminal or suspended transition SHALL produce
cancelled/cancellation_requested. Exactly one stopped transition may commit. A race
that loses the transactional transition SHALL return the already persisted run rather
than overwrite its outcome.

## Delegation

The root run's turn, total-token, tool-call, and absolute-duration budgets SHALL be
shared by the entire delegation tree. Each child SHALL also enforce its own resource
budgets. One model response or started tool call SHALL debit the child and every
active ancestor exactly once. A child cannot increase or reset an inherited budget.

Child completion contributes usage but does not complete the parent. A blocked or
needs-input child SHALL stop the active parent as the same suspended outcome with a
bounded child-run reference in reason detail. A failed, limit-exceeded, or cancelled
child SHALL preserve the existing fail-closed parent behavior while recording the
child's canonical outcome. Parent cancellation maps still-running descendants to
cancelled/ancestor_cancelled. Delegated trees remain non-resumable until SPEC-0021's
recovery boundary is superseded.

## Persistence, events, and inspection

Canonical lifecycle/outcome/reason and cumulative budget/usage state SHALL be updated
in the same transaction as each stopped run event. Usage increments and their source
model/tool event SHALL also commit atomically. Counter arithmetic SHALL be checked;
overflow SHALL fail closed with `accounting_error` and SHALL NOT wrap.

Stopped event names SHALL be:

```text
run.completed
run.blocked
run.needs_user_input
run.failed
run.limit_exceeded
run.cancelled
```

Each payload SHALL include canonical lifecycle, outcome, reason code, bounded reason
detail, cumulative usage, and configured budgets. It MAY include an output or artifact
reference only for completed runs. It SHALL NOT copy raw provider errors, credentials,
or unrestricted tool results. Existing events retain their stored shape.

Public store APIs SHALL expose typed lifecycle, outcome, reason, usage, budget, and
recovery-disposition values. JSON run/history/inspection output SHALL retain all
existing fields and add these canonical fields. Human inspection SHALL print them
without hiding the legacy status. CLI run/resume SHALL exit zero only for completed;
all other stopped outcomes exit nonzero after printing the durable record. Read-only
history and inspection SHALL remain free of resource loading, provider construction,
database migration, network access, secret resolution, and process startup.

## Recovery dispositions and checkpoints

Inspection and future workers SHALL derive one of these dispositions:

- `complete`: terminal/completed;
- `resume_eligible`: interrupted active, terminal/failed, or terminal/cancelled, only
  when all SPEC-0019 ownership, deadline, checkpoint, binding, and effect checks pass;
- `suspended`: blocked or needs_user_input; retain for explicit future continuation;
- `restart_required`: limit_exceeded or a safe run that cannot satisfy resume
  identity/deadline requirements; or
- `reconciliation_required`: an incomplete, denied, failed, or uncertain side effect
  after the recovery boundary.

Disposition derivation SHALL never downgrade `reconciliation_required` based on
outcome prose or a worker lease. Completed and limit-exceeded runs SHALL not reopen.
The first lifecycle implementation SHALL preserve current failed/cancelled resume
eligibility, but suspended runs SHALL reject `ctx agent resume` until continuation is
specified. A future contract may narrow legacy recovery only with a migration and
compatibility plan.

The Phase 3 checkpoint migration SHALL bump the checkpoint schema and persist the
configured budgets, cumulative counters, accounting completeness, canonical outcome
version, original deadline, and existing binding identity. Resume SHALL restore these
values without resetting them. Legacy checkpoints MAY resume under their existing
resource limits; they SHALL NOT fabricate historical token or tool usage. Adding a
new budget changes resource identity and therefore prevents unsafe reinterpretation
of an old checkpoint.

## Compatibility and non-goals

Existing resources, normal final responses, legacy status strings, direct CLI
commands, event ordering, artifact behavior, approval behavior, and conservative
side-effect recovery SHALL remain compatible. The lifecycle implementation SHALL NOT
add background workers, task tables, automatic retries, context compaction, model
summarization, recoverable tool observations, or application-specific control logic.

## Acceptance Criteria

- Migration tests upgrade representative four-status databases without changing old
  events/checkpoints and preserve old JSON fields.
- Store and runtime tests cover every valid lifecycle/outcome combination and reject
  invalid combinations and arbitrary reason strings.
- Existing agents still receive the same model request/tool schemas and complete from
  ordinary final text.
- Opt-in control-call fixtures produce blocked and needs-input outcomes; malformed or
  mixed control calls fail before any ordinary tool execution.
- Turn, duration, token, and tool-call limits stop with distinct reasons and retain
  counters across resume.
- Missing usage remains unknown without a token budget and fails closed when a token
  budget is configured.
- Delegation tests prove root and child counters cannot reset or widen ancestor
  budgets.
- CLI JSON/human inspection exposes canonical and compatibility fields, returns the
  specified exit status, and remains side-effect free.
- Recovery tests distinguish resume-eligible, suspended, restart-required, and
  reconciliation-required runs without replaying uncertain effects.
