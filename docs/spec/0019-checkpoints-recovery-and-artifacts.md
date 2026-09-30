# SPEC-0019: Checkpoints, Recovery and Artifacts

**Status:** Authoritative  
**Date:** 2026-09-30  
**Related:** [execution](0017-local-agent-execution.md), [developer tools](0018-developer-tools-and-approvals.md)

MCP-backed runs are excluded from checkpoint/resume as specified in
[SPEC-0020](0020-mcp-client-tools.md); external session recovery is not implemented. Delegating resources and child
runs are also excluded as specified in [SPEC-0021](0021-agent-delegation.md).

## Checkpoint contract

The runtime SHALL persist a versioned snapshot before each model invocation,
including the initial call and every call after a complete tool turn. A snapshot
contains run/workspace identity, agent version, next turn, the full model request
(messages, tool schemas and continuation data), and a non-secret binding digest.
Tool call IDs in the conversation reference durable invocation rows.

The binding SHALL cover the model registry's provider/model identity, selected
model definition (including credential environment variable name, never its
value), canonical database path, effective keyword candidate limit, current tool
schemas/descriptions, and exact host policy. The resource version covers prompt,
model alias, declared tools, limits and resource permissions. Changes to these
bindings SHALL prevent resume. Unrelated config changes need not invalidate it.
Future changes to runtime semantics that affect recovery SHALL bump the snapshot
schema version. Unsupported versions SHALL fail closed.

Snapshots SHALL contain a valid complete model conversation with no unresolved
tool calls, be at most 16 MiB serialized, and reference the original input and
prompt. Resume SHALL compare stored tool calls/results with durable completed
invocations. A checkpoint and its `checkpoint.created` event SHALL commit in one
transaction. The checkpoint bound limits stored size; serialization still uses
memory before checking it. Exceeding the bound after a tool turn fails the run;
that turn cannot be safely replayed from the previous snapshot.

Snapshots contain local project text, model output and opaque provider
continuation data. They are not redacted transcripts or encryption. Credential
values are never deliberately copied by the model adapter, but tools or model
content may contain secrets. Existing history storage protections and local
access controls apply. Retention/purge and encrypted checkpoint storage are not
implemented here.

## Ownership and resume

```text
ctx agent resume <run-id> [--json] [--non-interactive]
```

New runs and resumed runs SHALL hold an exclusive nonblocking OS lock at
`.ctx/runs/<run-id>/.lock` for their execution and final persistence. A competing
owner SHALL fail before reopening a run. Filesystem locks release on process exit
or cancellation; no stale PID guessing or forced takeover is used. The lock inode
SHALL never be unlinked by the runtime and its descriptor SHALL close on exec.

Ownership is tied to the canonical workspace root, which also scopes database
access. Different roots sharing a database cannot resume one another's runs.
Secure ownership/artifact filesystem operations currently support Unix only;
run/resume SHALL fail closed on unsupported hosts. History/inspect remain
available. Run IDs SHALL be canonical UUIDs. Runtime paths SHALL reject symlink
components and hard-linked lockfiles. New directories/files use 0700/0600 modes;
existing directory permissions are not changed.

Resume SHALL acquire ownership, validate the run and latest checkpoint, then
reopen it transactionally using its expected last event sequence. Concurrent
state changes SHALL reject that transition. Eligible states are running (after
an interrupted owner), failed or cancelled. Completed runs SHALL never reopen.
Previous terminal/resume events remain in append-only history; reopening clears
materialized error/output/completion time and appends `run.resumed`.

The runtime SHALL reject recovery if any tool, approval, artifact or other unsafe
activity occurred after the latest checkpoint. It SHALL also reject incomplete,
failed or denied invocations, even if a tool appears safe to repeat. This includes
a successful patch/process whose result was recorded but whose next checkpoint
was not committed. There is no automatic reconciliation or override switch.
Inspect the original run and actual workspace state, then start a new run when
manual reconciliation is needed. Resume does not stop or reconcile processes
left alive by a crashed owner.

Model-only activity after the snapshot may be retried because runtime tools have
not executed. That includes a lost final response or a response that requested
tools before any tool request was persisted. Repeating a provider call may cost
money and produce different text. Each `model.requested` event SHALL consume a
turn, including failed or interrupted calls; resuming SHALL NOT reset that count.
The original `created_at + timeout_seconds` wall-clock deadline SHALL remain in
force across resumes. Expired runs cannot resume; downtime consumes this budget.
Resources with the default 300-second limit therefore require recovery within
that original five-minute window. A new run is required after expiry.

Recovery validation failures SHALL leave the original run history unchanged;
filesystem ownership setup may create missing lock directories. Once reopened,
normal terminal/approval behavior applies. No past approval is reused. JSON
success/failure records follow `run`; preflight failures return an error without
claiming a new execution occurred.

OS locks coordinate cooperative runtime processes; they are not a security
boundary against arbitrary programs deleting runtime paths. `workspace.patch`
SHALL reject `.ctx/runs` files, including canonical aliases. Unsandboxed
`process.exec` and external programs can modify history or lockfiles and must not
be allowed to do so if recovery guarantees are required.

## Artifacts

Final model responses up to 64 KiB SHALL remain inline in the run record. Larger
responses up to 16 MiB SHALL be written to
`.ctx/runs/<run-id>/artifacts/<uuid>-output.txt`. The run's output SHALL become a
short relative-path, size and SHA-256 reference. Responses exceeding 16 MiB SHALL
fail rather than silently truncate. This slice externalizes large final responses;
conversation and successful tool payloads remain in the existing SQLite records
and snapshots required for recovery.

Artifact files SHALL be created exclusively under unique names, synced along with
the containing directory, and never overwritten by the runtime. Traversal SHALL
use no-follow directory descriptors and check for directory replacement. Only
successfully written/synced files may receive SQLite metadata. `artifact.created`
events SHALL store root-relative path, byte size and SHA-256 digest. Metadata
writes SHALL be workspace scoped, reject duplicate paths and terminal runs, and
reserve `artifact.*` against generic append. No additional metadata table is
required; the immutable event log is canonical.

Filesystem publication and SQLite metadata cannot be one atomic transaction. A
crash or write failure can leave an unreferenced partial or complete file. Such
files SHALL NOT be treated as committed artifacts. There is no automatic orphan
cleanup. A crash after metadata publication but before run completion SHALL
require reconciliation, preventing silent duplicate publication on resume.
Hashes permit integrity checking but are not continuously revalidated by inspect.

`ctx agent inspect` SHALL include an `artifacts` array in JSON and artifact
references in human output. It SHALL NOT read large artifact contents, load model
providers, or expose checkpoint conversations in its ordinary event output.

## Validation

Tests cover interrupted model calls, recovery of completed tool conversations
without replay, rejection after an interrupted side effect, owner conflicts,
resource/policy/schema mismatches, turn/deadline preservation, CLI recovery,
transactional state reopening, scoped immutable metadata, artifact integrity,
symlink/ancestor replacement and lock release. Abrupt future cancellation is used
to exercise missing terminal cleanup; these tests do not claim kernel-level
power-loss durability or automatic recovery of subprocess descendants.
