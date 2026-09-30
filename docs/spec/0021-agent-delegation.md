# SPEC-0021: Controlled Agent Delegation

**Status:** Authoritative  
**Date:** 2026-09-30  
**Related:** [resources](0015-agent-resources.md), [execution](0017-local-agent-execution.md), [permissions](0018-developer-tools-and-approvals.md), [recovery](0019-checkpoints-recovery-and-artifacts.md)

## Resource and invocation

A standalone resource may explicitly delegate to named resources:

```toml
[agent]
name = "planner"
model = "reasoning"
tools = ["search", "agent.invoke"]

[agent.permissions]
allow = ["read_only", "agent_delegate"]

[agent.delegation]
allow = ["researcher", "reviewer"]

[prompt]
system = "Use the allowed specialists to investigate the task."
```

The optional delegation table SHALL contain only `allow`, a list of at most 32
unique valid resource identifiers. Self-delegation SHALL be rejected. A nonempty
list and the declared `agent.invoke` tool SHALL be required together. Omitted or
empty delegation settings SHALL preserve existing resource hashes by being
omitted from serialization. Existing prompt-only agents do not become executable
children through this feature.

`agent.invoke` SHALL accept exactly `{ "agent": "name", "input": "task" }`.
Input SHALL be nonblank and at most 64 KiB. The advertised tool schema SHALL
include the allowed target names. Dispatch SHALL reject undeclared targets,
unavailable resources, ancestry cycles and excessive depth before starting the
invocation. Tool metadata SHALL require the new `agent_delegate` capability.
The default host policy permits this control capability, but resources must opt
in explicitly; default read-only resource permissions do not include it.

Delegation SHALL run sequentially in the parent's tool loop, through the normal
request/authorization/approval/start/result lifecycle. Each child receives a new
run ID, its own resource prompt and the explicit input. The parent's conversation
SHALL NOT be implicitly copied. Successful results contain child run ID, agent
name, status and final output (or its existing artifact reference). A child that
fails or is cancelled SHALL fail the invoking tool and parent run. Siblings are
not background jobs and no workflow/DAG scheduler is introduced.

## Catalog and authority

The CLI SHALL resolve the resource catalog once and initialize model aliases only
for the root and transitively allowed targets. The graph walk SHALL reject
missing targets/models and more than 256 reachable agents; unrelated provider
aliases SHALL remain uninstantiated. Library callers supply the catalog with
`AgentRuntime::with_resources`. Child invocation SHALL NOT rediscover files,
select another workspace/configuration, or accept model/permission overrides.

Every child's host policy SHALL be the intersection of its parent's effective
host policy and the parent's own resource permissions. The child's resource then
further restricts that policy. Denied capabilities cannot become approval-eligible
in a child. An approval requirement in any ancestor SHALL remain required even
if the child declares automatic allowance. The same approval handler is used,
but approval grants are never inherited or reused. This applies equally to
workspace writes, process execution, MCP startup/calls and further delegation.

Delegation permission alone SHALL NOT confer read, write, process, network or
external-side-effect authority. A parent intending a specialist to write must
itself declare the applicable write capability. Unsandboxed processes retain the
host-access limitations described in SPEC-0018 and SPEC-0020; delegation is an
authorization boundary, not an operating-system sandbox.

## Shared execution limits

The root's `max_turns` SHALL be a shared budget for all model attempts in the
entire tree, including the root. Each child also retains its own per-run turn
limit. An atomic counter SHALL be consumed before each model attempt; child
creation does not replenish it. Root final synthesis consumes a turn too. When
no shared turn remains, later tools in the same response SHALL NOT execute,
because no model attempt can consume their results.

The root's absolute deadline SHALL cover descendants. Each child deadline SHALL
be the earlier of its own creation time plus resource timeout and the inherited
ancestor deadline. Child deadlines cannot extend any ancestor's execution.
Maximum depth is four delegation edges: root depth 0 through child depth 4.
Calling any resource name already in the active ancestry SHALL be rejected.
These checks supplement the existing sequential 32-tool-per-turn limit.

Parent cancellation, timeout or fatal failure SHALL drop active child execution
and atomically finalize all still-running descendants in SQLite, including
unfinished tool invocations and pending approvals. Already-terminal children keep
their results. A parent cannot complete while a descendant remains running.
OS-process cleanup follows the existing direct-child termination guarantees;
crashes may leave external descendants alive.

## Durable lineage and inspection

An idempotently installed `agent_run_lineage` table SHALL retain run ID,
`parent_run_id`, `root_run_id`, depth and `parent_call_id`. Roots point to themselves
and have no parent. Older runs missing lineage are treated as roots.

Child creation SHALL atomically require a running same-workspace parent and a
started `agent.invoke` invocation; one child per parent call is permitted. It
SHALL write child metadata, child `run.created`, and parent `delegation.started`
in one transaction. Root creation retains its existing `run.started` event.
Generic event append SHALL reserve `delegation.*` against forged lifecycle data.
Child creation and terminal cleanup SHALL remain workspace scoped.

`ctx agent inspect` SHALL include `lineage` and direct `children` metadata in JSON
and their identities in human output. Child results/events are inspected through
the child's run ID. History still lists individual runs. Read-only inspection of
a database predating the lineage table SHALL return root/empty-child metadata
without migrating or writing the database.

## Recovery boundary

Delegating resources and delegated child runs SHALL NOT produce resumable
checkpoints in this slice. Tree budgets and in-flight parent/child state require
a coordinated recovery protocol that is not implemented. Runtime resume SHALL
reject these executions, and store-level reopening SHALL reject children and
runs that already have children. Ordinary local, non-delegating checkpoint
behavior remains available. Changes to default host capability policy may reject
older checkpoints whose saved policy no longer matches exactly.

## Validation

Tests cover strict resource declarations and unchanged default hashes, durable
child results/lineage, denied privilege escalation, inherited approvals, shared
turn exhaustion including later tools in the same response, cycle/depth limits,
parent deadline and cancellation, atomic child creation and terminal cleanup,
workspace isolation, duplicate calls, migration idempotence, legacy read-only
inspection, and rejection of independent tree reopening. Tests use scripted
models without live provider calls.
