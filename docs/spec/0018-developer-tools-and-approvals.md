# SPEC-0018: Developer Tools and Approvals

**Status:** Authoritative  
**Date:** 2026-09-30  
**Related:** [local execution](0017-local-agent-execution.md), [resources](0015-agent-resources.md)

## Permission boundary

The local runtime SHALL expose its built-in registry and the per-run MCP tools
defined in [SPEC-0020](0020-mcp-client-tools.md). Tool
capability metadata SHALL be explicit; missing or empty metadata SHALL deny
execution. The existing `Tool` trait defaults to unknown metadata, so extending
that trait does not silently grant local execution to existing Lua, Rust or MCP
tools. Lua/arbitrary Rust extension loading remains a later slice.

Effective access SHALL intersect registered tools, agent declarations, agent
permissions and a host-owned `RuntimePolicy`. Every required capability must be
allowed or approval-eligible in both policies. If either policy requires approval,
that invocation requires approval. A denied capability cannot be approved away.
The model cannot select or change host policy. Authorization and strict argument
validation SHALL happen before tool start; undeclared calls SHALL never dispatch.

The default host policy allows `read_only` and requires approval for
`workspace_write`, `process_execute` and `external_side_effect`. It denies other
tool capabilities.
Resource defaults remain read-only. A resource must explicitly declare privileged
capabilities, even to request approval. The library defaults to `DenyApprovals`;
a trusted embedding application may use `with_policy` to supply a narrower or
explicitly broader host policy and an `ApprovalHandler`. CLI resources and project
configuration cannot widen the host ceiling. Provider transport remains separate
from tool network permissions.

## Developer tool contract

All arguments SHALL be strict JSON objects; unknown fields are errors.

| Tool | Capability | Arguments and behavior |
|---|---|---|
| `workspace.read` | `read_only` | `path`: read an existing regular UTF-8 file, at most 1 MiB |
| `workspace.search` | `read_only` | `path`, nonempty literal `query` (up to 8192 bytes), `limit` 1–100 (default 100); recursive file search returns paths, one-based lines and bounded snippets |
| `git.status` | `read_only` | `{}`; porcelain status of the root repository |
| `git.diff` | `read_only` | `{}`; unstaged diff of the root repository |
| `workspace.patch` | `workspace_write` | `path`, nonempty `old_text`, `new_text`; replace exactly one matching occurrence in an existing UTF-8 file; source, replacement and result at most 1 MiB |
| `process.exec` | `process_execute` | `argv` (1–128 strings, at most 64 KiB total), `cwd` (default `.`), `timeout_seconds` 1–60 (default 30) |

Workspace paths SHALL be relative to the canonical bound root. Absolute paths,
parent components, `.git` metadata paths, and symlinks resolving outside the root
SHALL be rejected. Patching runtime `.ctx/runs` paths is forbidden. Existing paths only are supported;
patching does not create or
delete files. Patch SHALL reject ambiguous matches, including overlapping
matches; Unix hard-linked files SHALL be rejected. Checks are repeated at
execution, after approval. These checks do not provide OS isolation against
hostile concurrent filesystem replacement. Patching is not a multi-file atomic
transaction and a crash or I/O failure can leave uncertain file contents.

Search SHALL skip symlinks and `.git`, skip unreadable/non-UTF-8/oversized files,
return at most 100 matches with 1000-character snippets, and report truncation
when its traversal/result budget is reached. Traversal limits are 10,000 entries,
1000 files and 16 MiB of loaded text (the final file may add up to 1 MiB). Search
SHALL yield between files for cooperative cancellation.

Git commands SHALL use fixed argv, no shell, a cleared environment except PATH
and explicit Git controls, no global/system configuration, and disabled optional
index writes, fsmonitor, hooks, external diffs, text conversion and clean/process
filters. Local filter keys SHALL be enumerated and overridden before status/diff.
Git metadata for linked worktrees may live outside the bound working directory.
These tools require `.git` at the root and do not discover a parent repository.
They use the host Git executable and read repository configuration; they are not
a sandbox for a compromised executable or concurrently changing repository.

Processes SHALL receive explicit argv without an implicit shell. Their working
directory SHALL be inside the workspace. Standard input is closed; stdout and
stderr are each bounded to 128 KiB. Timeout/output overflow SHALL fail the tool;
normal nonzero exit SHALL be returned as `success: false` with `exit_code` and
output for the model to interpret. Timeout/cancellation terminates the immediate
child; descendant termination is not guaranteed.

`process_execute` deliberately grants arbitrary execution with the user's host
permissions and inherited environment. It can access files outside the root,
credentials, network, and subprocesses. The cwd check is not a process sandbox;
separate network/workspace capabilities do not constrain arbitrary commands.
The approval UI SHALL disclose this scope. Users needing OS isolation must supply
an isolated execution environment before enabling this capability.

## Approval lifecycle

Approval SHALL apply once to an exact requested invocation, including its stored
arguments. No run-wide or remembered grants are implemented. The store SHALL
atomically record `approval.requested`, then exactly one `approval.granted` or
`approval.denied`, correlated by run and call ID. Requested capabilities appear
in the request event; arguments remain in the invocation row. Generic event
append cannot forge `approval.*` lifecycle events.

Only requested tools in the bound workspace's running run may request or decide
approval. `start_tool` SHALL reject pending/denied approvals. Duplicate requests,
decisions, and cross-workspace changes SHALL fail without partial events.
Failure/cancellation SHALL atomically deny pending approvals and fail unfinished
tools before the terminal event. Explicit tool denial SHALL also close a pending
approval. Approval decisions remain visible in `ctx agent inspect` events.

The runtime SHALL persist approval before starting the tool. A denied request
fails that invocation and run. Granting approval does not guarantee a tool will
run or succeed; cancellation, validation and persistence errors can intervene.
Conservative recovery without uncertain side-effect replay is defined by
[SPEC-0019](0019-checkpoints-recovery-and-artifacts.md).

## CLI behavior

Interactive Unix CLI runs may ask for approval only when stdin and stderr are
terminals. The prompt SHALL show run/call identity, tool, capabilities and complete
JSON-escaped arguments, then require `yes` for that invocation. Control characters
SHALL remain escaped. Requests larger than 32 KiB of rendered details SHALL be
denied rather than asking about truncated arguments. Queued terminal input SHALL
be discarded before each prompt. Waiting SHALL be cancellable by the run deadline
or Ctrl-C without leaving a blocking input worker alive.

`--non-interactive`, piped input/output terminals, unavailable terminal access,
unsupported non-Unix terminal adapters, EOF or any other answer SHALL deny
approval. stdout JSON remains reserved for the final run record; prompts go to
stderr. The CLI does not offer an automatic approval switch.

Example resource (with an existing configured model alias):

```toml
[agent]
name = "developer"
model = "reasoning"
tools = ["workspace.read", "workspace.search", "git.status", "git.diff", "workspace.patch"]

[agent.permissions]
allow = ["read_only"]
require_approval = ["workspace_write"]

[prompt]
system = "Inspect the project and propose focused fixes."
```

Run `ctx agent run developer "Fix the selected issue"` from the workspace root.
Each requested patch requires a fresh approval. To enable commands, explicitly
add `process.exec` to tools and `process_execute` to `require_approval`.

## Validation

Tests SHALL cover policy intersection and host ceilings, per-call granted/denied
history before side effects, default denial, cancellation while awaiting approval,
concurrent decision serialization, terminal queued input, path and symlink escapes,
hard links, special files, ambiguous patches, process limits, Git helper/filter
suppression and linked worktrees. Existing runtime/retrieval/MCP tests remain
regression gates. Live provider calls are not required for these tests.
