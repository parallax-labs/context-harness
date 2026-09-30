# SPEC-0020: MCP Client Tools

**Status:** Authoritative  
**Date:** 2026-09-30  
**Related:** [local execution](0017-local-agent-execution.md), [permissions](0018-developer-tools-and-approvals.md), [recovery](0019-checkpoints-recovery-and-artifacts.md)

## Scope and configuration

The local agent runtime SHALL support explicitly configured stdio MCP servers as
another source of tools in its existing `ToolRegistry`. Existing MCP server,
retrieval and prompt interfaces SHALL remain independent of client sessions.

```toml
[mcp_servers.example]
command = "example-mcp-server"
args = []
timeout_seconds = 30
```

Server keys SHALL contain 1–64 ASCII letters, digits, underscores or hyphens.
`command` SHALL be nonblank, at most 4096 bytes and contain no NUL. `args` defaults
to empty and SHALL contain at most 128 strings, no NULs, and at most 64 KiB total.
`timeout_seconds` defaults to 30 and SHALL be between 1 and 60. Unknown fields
SHALL be rejected. Normal configuration loading SHALL validate declarations
without starting a server.

Only servers referenced by the executing agent's declared tools SHALL start.
Each run may use at most eight servers, with one session per selected server.
Sessions SHALL be isolated to the run and not pooled between runs. Processes
start in the bound workspace root, receive explicit argv without a shell, and
inherit the host environment. This slice does not provide HTTP/SSE transports,
OAuth, custom environment maps, alternate working directories or sandboxing.

## Discovery and names

A remote tool named `search` from server `example` SHALL appear as
`mcp.example.search`. Agent resources SHALL explicitly list the complete name.
Only those declared tools SHALL be advertised to the model, although discovery
may register additional tools internally. Built-in names SHALL remain unchanged.

The client SHALL use the pinned MCP SDK for initialization, typed protocol
messages and calls. Discovery SHALL support pagination, with at most 16 pages and
128 tools per server. Duplicate tool names, repeated/oversized cursors, invalid
names, and excess pages/tools SHALL fail discovery. Remote names SHALL contain
1–128 ASCII letters, digits, underscores, hyphens or periods. Cursors are limited
to 4096 bytes. Input schemas SHALL declare object type. Tools requiring MCP task
execution SHALL be rejected because task orchestration is not implemented.

Each adapter SHALL expose the remote description and input schema through the
existing `Tool` trait. Advertised annotations are untrusted; a remote
`readOnlyHint` SHALL NOT grant read-only capability or suppress approval. Schema
content is supplied to the model; the runtime enforces an object argument envelope
and byte limit, while the remote server validates its full JSON Schema. This
slice does not introduce a separate local JSON Schema evaluator.

## Permission and startup boundary

Both server startup and every external tool call SHALL require
`process_execute` and `external_side_effect` in the agent and host policies.
The default host policy makes both approval-required; a resource cannot grant
itself automatic startup. Trusted embedding applications may explicitly supply
an allowing policy, as for existing developer tools.

The runtime SHALL preflight all declared tools and required capabilities before
starting any server. For each selected server it SHALL create a synthetic
`runtime.mcp.start.<server>` invocation, recording command, argv and workspace
root as arguments. Authorization and any approval SHALL precede `tool.started`
and process spawning. Initialization/discovery success SHALL complete that
invocation; connection/discovery errors SHALL fail it and the run. The synthetic
invocation is an internal lifecycle operation, not a model-callable tool.

Discovered tool calls SHALL use the existing request/approval/start/result path.
Every required approval applies only to its invocation; approving startup does
not approve subsequent calls. Non-interactive CLI runs SHALL deny required
approvals. An unchanged read-only agent SHALL fail before server startup.

Launching a server grants arbitrary host process execution. A server may act
during startup, between calls, or contrary to its tool description; it inherits
access to environment credentials and may start children or use the network.
Per-call approvals control which RPCs the runtime sends, not what an already
running unsandboxed server can do. The approval UI SHALL disclose this broad
process scope. Use only trusted servers or an externally isolated environment.

## Transport, errors and lifecycle

Stdio frames SHALL be newline-delimited JSON with at most 1 MiB per incoming or
outgoing frame. Malformed, oversized, or closed streams SHALL end the connection;
there is no reconnect or retry. Initialization, each discovery request, and each
tool call SHALL have the configured timeout, also bounded by the overall run
deadline. Tool arguments SHALL be JSON objects of at most 64 KiB and serialized
results SHALL be at most 1 MiB. Limits bound individual messages, not all memory
or unsolicited traffic from a malicious process.

The client SHALL advertise no sampling, roots or elicitation capabilities and
reject server-initiated requests. Notifications SHALL be ignored; this slice does
not refresh discovery in response to tool-list changes. Task-only tools are not
supported. Server tool results with `isError: true` SHALL fail the invocation;
successful results are passed as serialized MCP result JSON to the model.

The process owner SHALL remain in the run's session guard, separate from the
SDK's background transport task. Normal completion/error SHALL close the session
and terminate/reap the direct child with bounded waits. Dropping a session on
cancellation or other early exits SHALL cancel the service and request direct
child termination, with asynchronous reaping handled by the process runtime.
There is no descendant-process sandbox or guarantee that grandchildren stop.
Shutdown errors do not retroactively change a completed tool's recorded result.

Server stderr is discarded. Runtime history SHALL use fixed failure categories,
not arbitrary transport/server error text. Arguments, successful results and
command lines remain local project history. Do not put secrets in command-line
arguments; inherited environment values are not deliberately copied into history.
The CLI does not install SDK debug logging; embedding applications that enable
verbose SDK tracing must account for its possible request/response payload logs.

## Checkpoints and compatibility

MCP-backed runs SHALL NOT write conversation checkpoints or accept resume in
this slice. A conversation cannot reconstruct an external process session, and
restarting a server may repeat side effects. After interruption, inspect the run
and external state before starting a new run. Local-only checkpoint/recovery
behavior remains as specified in SPEC-0019. Exact host policy comparison may
reject an older checkpoint after changes to the default approval capabilities.

Loading configurations, inspecting history, and listing/showing static agent
resources SHALL NOT connect to MCP servers. The existing MCP server endpoints
and legacy prompt behavior SHALL remain unchanged.

## Example resource

With the server above and an existing model alias:

```toml
[agent]
name = "external-researcher"
model = "reasoning"
tools = ["search", "mcp.example.search"]

[agent.permissions]
allow = ["read_only"]
require_approval = ["process_execute", "external_side_effect"]

[prompt]
system = "Use indexed context and the external search tool to answer the task."
```

Run `ctx agent run external-researcher "Investigate the issue"` from the workspace.
The interactive CLI asks separately about server startup and each external call.
Without a terminal or with `--non-interactive`, required approvals are denied.

## Validation

A standard-library-only Python fixture supplies a local MCP server; tests make no
network calls. Tests cover paginated discovery, namespace/schema projection,
normal model/tool history, startup denial without spawning, per-call approval
regardless of read-only hints, explicit host policy, unused servers remaining
idle, malformed/oversized frames, duplicate tools/cursors, tool limits, startup
and call timeouts, disconnects, server errors, callback rejection, non-interactive
CLI denial, cancellation, and child cleanup.
Existing agent, retrieval, checkpoint and MCP server suites remain regressions.
