# SPEC-0017: Local Agent Execution Spec

**Status:** Authoritative  
**Date:** 2026-09-26  
**Related:** [agent resources](0015-agent-resources.md), [model runtime](0016-model-runtime.md), [execution plan](../design/0011-local-agent-runtime-execution-plan.md)

## Scope

`ctx agent run` SHALL execute a standalone static agent resource using its model
alias and declared tools. The initial runtime SHALL support local keyword search
and indexed document retrieval. Legacy TOML/Lua/Rust prompt interfaces and existing
MCP server behavior SHALL remain available without acquiring an execution loop.

Developer tools, capability metadata and interactive approvals are defined by
[SPEC-0018](0018-developer-tools-and-approvals.md). External stdio MCP tools are
defined by [SPEC-0020](0020-mcp-client-tools.md). Delegation and streaming remain
later slices. Recovery is defined by
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md).

## Execution

The runtime SHALL create a durable run before executing an agent. The record
includes workspace identity, agent name/resource version, model alias and input.
Input SHALL contain non-whitespace text and SHALL be no larger than 64 KiB.

After validating declared tool availability and policy, it SHALL record
`context.resolved`, then send the static system prompt and user input to the model.
Each response SHALL either complete the run or request tools. Tool results SHALL
be appended to the conversation and supplied to the next model turn. Calls within
one response SHALL execute sequentially in provider order.

Completed model responses SHALL produce `run.completed` and persist their text as
run output. Model failures, output truncation, refusal/filtering, invalid tool
arguments, unavailable permissions, fatal tool failures, and execution limits
SHALL produce a failed run. The runtime SHALL NOT silently retry model or tool
calls. Tool exception text SHALL be replaced with a fixed error category rather
than copied into history.

`max_turns` SHALL count model invocations. The runtime SHALL NOT execute tool calls
from the final allowed turn, because no turn remains to consume their results.
Each turn SHALL permit at most 32 tool calls. Serialized tool results SHALL be
limited to 1 MiB before being persisted or passed to the model. This limits stored
and transmitted results, not peak memory during document loading/serialization.

The agent's timeout SHALL wrap context resolution, model calls, tool calls and
associated event writes after run creation. Timeouts are cooperative Tokio
cancellation; final persistence cleanup runs after the execution deadline. CLI
Ctrl-C SHALL request cancellation and persist `run.cancelled`. Library consumers
MAY supply the same cancellation through a watch receiver. Closing that channel
without a cancellation value SHALL NOT cancel a run.

If final persistence fails, the call SHALL return a storage error with the run ID;
it SHALL NOT claim that terminal state was durably recorded. Abrupt process death
can leave a run marked running. Recovery eligibility and ownership are defined by SPEC-0019.

## Workspace binding

The current single-workspace CLI SHALL use its canonical working-directory root.
The run workspace key SHALL be `local-` followed by the SHA-256 hex digest of that
canonical path's OS-encoded bytes. This avoids ambiguity between same-named
folders and preserves identity through symlink spellings on the same host.
Registered multi-workspace selection is not added by these commands.

The runtime SHALL resolve relative DB paths beneath the bound root and use that
same configured database for context and execution persistence. Moving a root
changes its local workspace key. Root migration requires an explicit later
workflow; runs SHALL NOT silently move between roots. If two roots deliberately
share one database, their run histories remain logically isolated, while their
indexed context is shared according to that explicit configuration.

## Tools and permissions

The runtime SHALL own built-in `ToolRegistry` adapters for `search`, `get` and
the developer tools specified in SPEC-0018. Per-run external MCP adapters follow
SPEC-0020. All implement the existing `Tool` trait. The runtime SHALL NOT load
Lua/Rust extensions or trust an arbitrary tool's name as proof that it is read-only.

The `search` and `get` adapters SHALL use a separate SQLite connection pool opened read-only, with
`query_only` enabled and no database creation. Keyword search SHALL reuse the core
search algorithm and SQLite store; get SHALL reuse core document retrieval.
Runtime initialization/history writes remain intentional application operations
outside model tool permissions.

Declared tools SHALL exist in that restricted registry, with capabilities permitted
by both agent and host policy as defined in SPEC-0018. Approval-eligible tools may
be advertised, but SHALL NOT execute before the required per-call approval.
Unknown capabilities SHALL NOT be implicitly granted. Runtime dispatch
SHALL check declarations, permissions, availability and arguments again before
execution. Undeclared calls rejected by the model boundary SHALL NOT dispatch.
The configured model's provider transport is separate from tool capabilities.

Arguments SHALL be strictly typed JSON objects with unknown fields rejected:

| Tool | Accepted arguments |
|---|---|
| `search` | Nonempty query of at most 8192 bytes; mode absent or `keyword`; integer limit 1–100 (default 12); optional filters with source (at most 1024 bytes) and valid canonical YYYY-MM-DD `since` |
| `get` | Nonempty document ID of at most 128 bytes, without a workspace prefix |

Workspace selectors SHALL be rejected. Date validation SHALL occur before search,
even for an empty index. Keyword candidates SHALL be clamped to 1–1000 using the
configured retrieval candidate limit.

These adapters SHALL NOT invoke embedding services, rebuild vector sidecars or
start connector processes. `sources` is not enabled because the existing source
listing invokes Git. Semantic/hybrid retrieval and sources require future
capability-aware adapters.

## Durable tool lifecycle

Tool transitions SHALL be materialized in `tool_invocations`, with matching
ordered events committed atomically:

- Request: create invocation and `tool.requested`.
- Start: requested → started with `tool.started`.
- Complete/fail: started → completed/failed with the corresponding event.
- Deny: requested → denied with `tool.denied`.

Call IDs SHALL be unique within a run. Invalid transitions SHALL leave neither a
new event nor a state update. All APIs SHALL enforce the run's workspace binding
and reject writes to terminal runs. Generic event append SHALL reserve `tool.*`
for these transactional APIs, alongside the existing lifecycle namespaces.

Completing a run while tools are unfinished SHALL fail. Failing/cancelling a run
SHALL atomically mark requested/started tools failed, append their failure events,
and then append the terminal run event. A cancelled model call can leave an
unmatched `model.requested` event as documented in SPEC-0016.

Invocation rows SHALL retain arguments and successful results as local execution
data. Their events SHALL retain call identity and status rather than copying
arguments/results. Run input and final output are also persisted. These records
can contain project content; this slice does not add a retention/purge policy.
Versioned conversation snapshots and conservative resume eligibility are defined
by SPEC-0019. Large final output artifacts follow that spec as well.

## CLI

```text
ctx agent run <name> <input> [--json] [--non-interactive]
ctx agent resume <run-id> [--json] [--non-interactive]
ctx agent history [--limit N] [--json]
ctx agent inspect <run-id> [--after-sequence N] [--limit N] [--json]
```

`run` SHALL print its durable record as JSON when requested; human output includes
run ID, status, final output or failure. It SHALL exit nonzero for failed/cancelled
runs, while preserving the JSON run record on stdout when creation succeeded.
Interactive approval follows SPEC-0018; `--non-interactive` SHALL deny
approval-required calls and never silently approve anything. Setup failures before creation need not have a run ID.

History SHALL default to 20 runs, in most-recent-first order. Inspect SHALL default
to 200 events, ordered by sequence, with an exclusive `after-sequence` cursor.
Both limits SHALL accept 1–1000; the event cursor SHALL be nonnegative.

Inspection JSON SHALL contain `run`, `events`, `tool_invocations` (status metadata
only), `artifacts` metadata and `next_after_sequence`. A full event page SHALL return its final sequence
as a cursor; the next page can be empty. The history output SHALL be an array of run
records. Tool arguments/results remain in SQLite and are omitted from CLI inspect
output to avoid embedding large document payloads in inspection output.

History/inspect SHALL use read-only connections and SHALL NOT initialize a missing
database. History with no runtime tables SHALL return an empty list; inspect SHALL
fail for an absent or cross-workspace run. Neither command SHALL load agent resource
files or instantiate model providers.

## Try the local loop

Add a model alias to the normal workspace config:

```toml
[models.demo]
provider = "fake"
model = "demo"
```

Create `.ctx/agents/researcher.toml`:

```toml
[agent]
name = "researcher"
model = "demo"
tools = ["search", "get"]

[prompt]
system = "Use indexed project context to answer questions."
```

Then run `ctx agent run researcher "Explain this project" --json`. The fake
provider returns an explicitly synthetic answer without external model calls; it
demonstrates run persistence and inspection. Real retrieval turns require a model
that requests tools. Configure an OpenAI alias as described in SPEC-0016 to use
that adapter. Automated tests use a scripted model that searches, retrieves a real
indexed document and returns a grounded answer over three turns.

## Validation

Runtime tests cover search/get iteration against a temporary indexed database,
strict arguments, policy rejection, turn limits, timeout/cancellation, tool
failures, terminal invocation cleanup, and logical workspace isolation. CLI tests
cover run/history/inspect, JSON failure records, event pagination and read-only
inspection without resource/model initialization. Existing retrieval, agent and
MCP tests remain regression gates.
