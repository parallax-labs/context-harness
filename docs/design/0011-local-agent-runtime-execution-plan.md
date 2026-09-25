# DESIGN-0011: Local Agent Runtime Execution Plan

**Status:** Planning  
**Date:** 2026-09-25  
**Author:** Context Harness contributors  
**Related:** [DESIGN-0010](0010-local-agent-runtime.md), [ADR-0024](../adr/0024-local-agent-runtime.md)

## Context

Execute DESIGN-0010 incrementally while keeping ingestion, retrieval, existing
agents, and MCP prompt projection working. The first deliverable is persistence,
with no LLM, provider credentials, or network service required.

## Proposal

Keep early runtime code in the existing application crate. SQLite runtime history
is an application responsibility, exposed through `SqliteAppStore::agent_runs`;
it does not expand the portable core search `Store` trait. Reuse the application's
pool and migrations. Extract an internal agent crate when the loop and provider
interfaces establish a useful dependency boundary.

Runs bind to the resolved workspace ID at creation. The store scopes reads and
writes to that ID. Event sequence allocation, run state updates, and checkpoint
creation use transactions. Checkpoint state is versioned JSON; runtime code will
validate its conversation/resource/policy schema before resume. Credentials must
never be embedded in resource snapshots or event payloads.

## Implementation Plan

Each row is a reviewable delivery slice in dependency order. Add executable
acceptance tests and update this checklist as each slice lands.

| Slice | Deliverables | Acceptance gate | Status |
|---|---|---|---|
| 1. Persistence | Four runtime tables; workspace-bound run creation/history; paginated ordered events; terminal transitions; versioned checkpoint save/load | Synthetic run survives reopen/migration; concurrent appends have unique ordered sequences; failures roll back; other workspaces cannot access it | Complete |
| 2. Agent resources | Standalone TOML definitions; global/workspace precedence; model references, limits and policy parsing; extend existing agent list/show/validate | Old TOML/Lua/Rust agents still resolve; malformed/unknown configuration fails clearly; workspace override tests | Complete |
| 3. Model boundary | Provider-neutral request/response/tool/usage types; registry; deterministic fake; one production provider | Fake and provider contract tests cover tool calls, failures and usage; credentials resolved from environment and excluded from history | Complete |
| 4. Execution loop | Resolve agent/context/workspace; model/tool iterations through ToolRegistry; ctx agent run/history/inspect; JSON output; turn/time/cancellation limits | Fake model searches/gets context over multiple turns and completes with persisted history; terminal errors recorded | Pending |
| 5. Developer capabilities | Read/search, git status/diff, patch/process tools; capability metadata; policy intersection; persisted approvals | Read-only policy blocks writes/processes regardless of prompt; path escape tests; non-interactive execution never silently approves | Pending |
| 6. Resume/artifacts | Typed checkpoint schema; resume command; interruption handling; artifact files and metadata | Crash tests around model calls and tool side effects; no automatic replay of uncertain non-idempotent tool execution; workspace/version checks | Pending |
| 7. MCP client | External server config/lifecycle, tool discovery adapters and namespacing | External fixture tool runs through the same registry/policy/event path; timeout/disconnect handling | Pending |
| 8. Delegation | Controlled agent.invoke; parent/root run IDs; depth/turn/time budgets; inherited permission ceilings | Child execution is attributable and bounded; child cannot increase parent privileges | Pending |
| 9. MCP compatibility | Project resource-backed executable agents as stateless MCP prompts | Existing prompt clients and Lua/Rust resolution remain compatible; full regression and end-to-end first-target demo | Pending |

Slices 2 and 3 depend on slice 1; slice 4 joins them. Basic declared-tool
allowlisting and deny-by-default treatment of privileged/unknown capabilities
must ship with slice 4, before developer tools in slice 5. MCP compatibility
regressions run throughout; slice 9 is the final resource projection gate.
The optional local queue follows these milestones only if direct invocation
proves insufficient.

### First slice boundaries

- Add `agent_store` in the application crate and idempotent schema installation.
- Store run metadata, start/terminal events, arbitrary non-lifecycle runtime
  events, and opaque versioned snapshots. Timestamps use Unix milliseconds.
- Reserve lifecycle/checkpoint events for atomic state-changing APIs.
- Add the tool invocation table now; transactional invocation lifecycle APIs
  arrive with the tool loop (slice 4), when call/result types are defined.
- Keep terminal runs immutable for now. Resume must define explicit transitions
  for interrupted runs; it must not rewrite completed history.
- No CLI run command is advertised until it can actually execute an agent.

## Alternatives Considered

- A new crate immediately adds dependency churn before runtime types stabilize;
  keep the module boundary extractable instead.
- Expanding the core Store trait couples portable retrieval consumers to runtime
  execution. Use an application-level facade over the same database.
- Reconstructing all state from events complicates resume. Retain both ordered
  events and snapshots as DESIGN-0010 specifies.

## Open Questions

Resolve these before their dependent implementation slices:

1. The first provider is OpenAI Responses, with non-streaming tool calls and
   structured output. [SPEC-0016](../spec/0016-model-runtime.md) defines the
   implemented boundary. Streaming remains a later interface/transport extension.
2. Resource hashing, replacement rules and legacy compatibility are resolved by
   [SPEC-0015](../spec/0015-agent-resources.md). Executable resource projection
   into MCP remains slice 9.
3. Define redaction, retention and artifact limits before recording production
   model/tool payloads. The initial storage API accepts caller-provided JSON and
   does not claim automatic redaction.
4. Specify approval trust boundaries for Lua and external MCP tools before slices
   4–5. Unknown tools must not be classified as read-only by default.
5. Define checkpoint schema, interruption states, ownership/locking and uncertain
   tool-result reconciliation before slice 6; a snapshot alone is not safe resume.

## Validation

Use temporary file-backed SQLite databases so reopen, transactions, and concurrent
writers exercise the production storage mechanism. Run persistence tests plus
existing app-store/retrieval tests, workspace tests, formatting and Clippy.
Provider-dependent acceptance will use deterministic fixtures in CI; live model
smoke tests remain explicit and credential-dependent.

### Slice 1 verification (2026-09-25)

- `cargo test --workspace --no-default-features`: 196 passed, 3 performance
  probes ignored. Existing ingestion, retrieval, CLI and MCP tests pass.
- `cargo test -p context-harness --no-default-features --test agent_store`:
  4 passed, including the final checkpoint-insert rollback assertion.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo clippy --workspace --all-targets --no-default-features`: completed;
  the existing `chunks_exact_to_as_chunks` warning in core `embedding.rs:50`
  remains. The strict `-D warnings` run stops on that warning; no warnings were
  reported for the new persistence code.
- Default embedding backends were not built in this slice; runtime persistence
  introduces no embedding or provider dependency.

### Slice 2 decisions and verification (2026-09-25)

- Added strict standalone TOML resources with model aliases, execution limits,
  capability declarations, provenance and SHA-256 content versions.
- Workspace replacement requires `agent.override = true` and replaces the whole
  definition. Explicit/pinned configs load only config-adjacent `agents` files.
  Legacy name collisions fail combined catalog validation rather than silently
  replacing existing prompt agents.
- Added `agent show`, `agent validate`, JSON list/show output and static prompt
  previews through `agent test`. Targeted legacy Lua tests retain their original
  behavior even when unrelated definitions are broken.
- Added [SPEC-0015](../spec/0015-agent-resources.md) as the authoritative contract.
  Provider/tool availability and runtime authorization are not yet asserted by
  declaration validation. Existing MCP registration is unchanged.
- `cargo test --workspace --no-default-features`: 207 passed, 3 ignored
  performance probes; 11 new resource tests included.
- `cargo clippy --workspace --all-targets --no-default-features`: completed with
  only the existing core embedding warning recorded above.
- Formatting and diff whitespace checks passed. Default embedding backends were
  not built in this slice.

### Slice 3 decisions and verification (2026-09-25)

- Added the public `agent_model` library module: neutral request/response types,
  `ModelProvider`, registry, typed errors, and deterministic scripted fake.
- Added OpenAI Responses with caller-owned history, function calls and results,
  opaque reasoning continuation, structured output, usage and explicit finish
  reasons. No default model is imposed; aliases retain configured model IDs.
- HTTP calls have bounded response size/time and no automatic retries or
  redirects. Credentials load lazily from environment. Tests use local fixtures;
  no live model call was made.
- Recorded calls bind to existing workspace/run/model identity and persist
  correlated request/response/failure metadata. Raw prompts, generated text,
  continuation items and HTTP errors are excluded from events. Conversation
  persistence and safe recovery remain the checkpoint/runtime layer's job.
- Added [SPEC-0016](../spec/0016-model-runtime.md). Streaming is deferred;
  initial generation returns complete responses. The model API can be invoked
  directly from Rust without an agent loop or a new CLI command.
- `cargo test --workspace --no-default-features`: 221 passed, 3 ignored
  performance probes, including 8 adapter tests and 6 model/store integration tests.
- Final focused adapter tests passed after preserving original tool names in
  wire descriptions. Formatting and diff checks passed.
- Clippy completed with only the existing core embedding warning. Default
  embedding backends and live provider calls were not exercised.

Next: slice 4, the local agent loop and `ctx agent run/history/inspect`, starting
with built-in context tools, static resources, explicit tool allowlisting and
bounded execution. Tool invocation lifecycle APIs will join the existing run
store; developer tools and safe resume remain later slices.
