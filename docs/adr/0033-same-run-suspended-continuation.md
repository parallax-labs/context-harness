# ADR-0033: Same-Run Suspended Continuation

**Status:** Accepted
**Date:** 2026-10-12

## Context

Context Harness can stop a durable agent run as `blocked` or
`needs_user_input`, but it deliberately rejects those runs from ordinary crash
recovery. DESIGN-0014 and DESIGN-0018 left open whether a later answer or
environment update should reopen the same run or create a linked replacement.

The decision must preserve the authority boundaries already established for
durable execution. A run owns its objective, transcript, usage, budgets,
checkpoints and outcome. A background task owns scheduling and has exactly one
immutable run identity. Delegation lineage represents an agent invoking another
agent, not successive attempts at one objective. No cross-run `Session` authority
exists.

Suspension also differs from a crash or ordinary terminal failure. Waiting for a
person or external condition is intentional and may last much longer than the
configured execution timeout. Counting that wait against the original absolute
deadline would make explicit continuation unusable, while resetting budgets in a
replacement run would discard the limits and audit trail that suspension is meant
to preserve.

Finally, suspension is requested through a runtime control tool call. A valid model
request cannot append a user message while that call is unresolved, and provider
continuation data must remain attached to its original assistant message.

## Decision

Explicit suspended continuation SHALL reopen the same run identity. It SHALL be a
separate operation from crash recovery and SHALL require a nonempty bounded input
from a trusted host. It SHALL NOT create a replacement or child run, mutate task
ownership, or introduce a `Session` abstraction.

The runtime SHALL preserve the assistant control-call message that caused
suspension. On continuation it SHALL resolve that call with a provider-neutral,
runtime-authored tool-result envelope containing the trusted continuation input and
the typed suspension context. The pair becomes an ordinary completed transcript
exchange before another model request. It does not create or execute an ordinary
tool invocation and does not consume the tool-call budget.

Continuation SHALL preserve cumulative model-turn, token and ordinary-tool usage.
The duration budget SHALL also remain cumulative, but time spent in the canonical
`suspended` lifecycle SHALL not consume it. Context Harness SHALL extend the
effective deadline by exactly the persisted suspended interval; crash downtime and
time spent in any other lifecycle retain their existing treatment.

Continuation SHALL use the existing exclusive execution lock and fail-closed
resource, model, provider, tool, policy, checkpoint and effect validation. The
suspension exchange, continuation input, checkpoint, lifecycle transition and audit
event SHALL become durable before the next provider call. Repeated or racing
requests SHALL never append the input twice.

The first implementation SHALL support standalone non-delegating, non-MCP runs
only. Workers SHALL not continue runs automatically. A terminal task linked to a
suspended run remains terminal scheduling history while the same authoritative run
may later continue through an explicit host operation.

## Alternatives Considered

### Create a linked replacement run

A new run would provide a simple fresh deadline, but it would split one objective's
transcript and accounting, conflict with the one-run-per-task invariant and overload
delegation lineage with retry semantics. Preserving cumulative budgets across the
link would require a new cross-run authority resembling the deferred `Session`
abstraction.

### Reuse ordinary crash resume unchanged

Crash resume restores the latest completed checkpoint and intentionally counts
downtime against the original absolute deadline. It has no trusted input and cannot
resolve the control call that caused suspension. Combining the operations would
weaken their distinct safety preconditions and make CLI intent ambiguous.

### Append the continuation as a user message

The preceding assistant response contains a pending control tool call. Appending a
user message would violate the model-request conversation contract and would either
drop or detach opaque provider continuation data. A runtime-authored result for the
pending control call preserves the valid provider-neutral exchange.

### Reset or replace the duration budget

A fresh timeout would let repeated suspensions widen execution without bound. Keeping
the unmodified absolute deadline would charge intentional waiting and usually make a
later continuation impossible. Excluding only persisted suspended intervals retains
the configured amount of execution time without granting more of it.

### Introduce a durable session

The current run already owns all state needed for this use case. A session would add
cross-run identity, retention and budget questions without a demonstrated
requirement.

## Consequences

- One run ID remains the complete audit trail across multiple suspension and
  continuation cycles.
- Continuation requires additive persistence for the pending control exchange and
  accumulated suspended duration.
- Provider continuation data remains opaque and attached to the assistant message
  that produced it.
- Existing crash resume behavior and deadlines remain unchanged.
- Task scheduling records do not become execution authorities again after reaching
  terminal `run_stopped`.
- Delegated trees and MCP-backed runs remain unsupported until their independent
  recovery boundaries are superseded.
- A later cross-run conversational feature still requires a separate decision about
  session identity and authority.

## References

- [PRD-0014](../prd/0014-durable-agent-runs.md)
- [DESIGN-0014](../design/0014-structured-agent-run-state.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [ADR-0028](0028-typed-run-outcomes-and-compatibility.md)
- [ADR-0029](0029-durable-agent-task-ownership.md)
- [ADR-0031](0031-deterministic-bounded-context-projection.md)
- [SPEC-0030](../spec/0030-suspended-run-continuation.md)
