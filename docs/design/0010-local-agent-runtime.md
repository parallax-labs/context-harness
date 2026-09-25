# DESIGN-0010: Context Harness Local Agent Runtime

**Status:** Proposed  
**Date:** 2026-09-24  
**Scope:** Context Harness evolution from context substrate into a local developer agent harness

## Summary

Context Harness currently provides a strong local substrate for AI-assisted development:

- local SQLite/WAL persistence;
- connector-driven context ingestion;
- hybrid retrieval;
- workspaces;
- `ToolRegistry`;
- `AgentRegistry`;
- TOML, Lua, and Rust extensions;
- MCP tools and prompts;
- extension registries;
- a single `ctx` binary.

However, Context Harness currently stops at the model boundary.

Its agents resolve prompts and tool scopes, then an external client such as Claude Code, Cursor, or another MCP client owns:

- model invocation;
- the agent loop;
- tool-call iteration;
- execution history;
- checkpoints;
- resume;
- delegation.

This design adds an **optional local agent runtime** to Context Harness.

The resulting product becomes:

> A local-first developer agent harness that owns context, agent definitions, tool execution, model execution, and durable run history while remaining interoperable with external MCP clients.

The primary artifact remains:

```text
ctx
```

No distributed infrastructure is required.

Except for configured LLM providers and explicitly configured external tools, execution remains local.

---

# 1. Current Architecture

Today the primary data flow is:

```text
Connectors
    ↓
Normalization
    ↓
Chunking
    ↓
Embeddings
    ↓
SQLite
    ↓
Query Engine
    ↓
CLI / MCP
    ↓
External Agent Client
```

Agent handling currently looks approximately like:

```text
AgentRegistry
    │
    ├── TOML Agent
    ├── Lua Agent
    └── Rust Agent
            │
            ▼
       AgentPrompt
            │
            ▼
       MCP Prompt
            │
            ▼
   Cursor / Claude / etc.
            │
            ▼
       model + loop
```

The important current implementation points are:

```text
crates/context-harness/src/agents.rs
crates/context-harness/src/agent_script.rs
crates/context-harness/src/tool_script.rs
crates/context-harness/src/traits.rs
crates/context-harness/src/mcp.rs
crates/context-harness/src/app_store.rs
```

This proposal should reuse those boundaries rather than introduce a parallel application.

---

# 2. Target Architecture

The new execution path is:

```text
                    Context Harness

             Declarative Agent Resource
                       │
                       ▼
                  Agent Runtime
                       │
          ┌────────────┼─────────────┐
          ▼            ▼             ▼
       Context        Model         Tools
          │            │             │
          ▼            │      ┌──────┴───────┐
     SQLite/FTS        │      │              │
     embeddings         │   built-in       MCP
                        │    tools        providers
                        │
                        ▼
                   LLM Provider
                        │
                        ▼
                  Agent Runtime
                        │
                        ▼
                SQLite Event Log
```

The existing path remains valid:

```text
Context Harness
      ↓
MCP
      ↓
Claude / Cursor / Codex
```

The two modes coexist.

---

# 3. Product Model

Context Harness becomes capable of operating in two roles.

## Context Server

Existing behavior:

```bash
ctx serve mcp
```

Context Harness exposes tools and prompts to another agent harness.

## Agent Harness

New behavior:

```bash
ctx agent run reviewer \
  "Review the current changes"
```

Context Harness itself owns:

```text
agent resolution
context construction
model calls
tool calls
permissions
execution history
checkpointing
resume
```

The same context engine and tool registry support both modes.

---

# 4. Architectural Principle

The core rule is:

> **Code implements capabilities. Declarative resources compose capabilities.**

Rust/Lua/MCP tools implement things that can actually happen.

Agent definitions decide:

- which model to use;
- which tools are available;
- what instructions govern the agent;
- what context is available;
- execution limits;
- permissions;
- which other agents may be delegated to.

Example:

```toml
[agent]
name = "reviewer"
description = "Reviews changes against project conventions"
model = "reasoning"

tools = [
  "search",
  "get",
  "workspace.read",
  "git.diff",
  "tests.run",
]

[agent.execution]
max_turns = 12
timeout_seconds = 300

[agent.permissions]
mode = "read-only"

[prompt]
system = """
You are a senior software engineer reviewing the current change.

Use project context before making assumptions.
Prefer concrete findings over stylistic opinions.
"""
```

No custom Rust `ReviewerAgent` should be required.

---

# 5. Declarative Agent Resources

Move executable agents toward standalone resources:

```text
.ctx/
├── agents/
│   ├── reviewer.toml
│   ├── implementer.toml
│   └── architect.toml
│
├── policies/
│   ├── read-only.toml
│   └── workspace-write.toml
│
└── config.toml
```

Global agents may also exist in the existing XDG Context Harness configuration hierarchy.

Resolution should follow the existing workspace model:

```text
built-ins
    ↓
global resources
    ↓
workspace resources
```

Workspace resources override global resources where explicitly allowed.

---

# 6. Existing Agent Compatibility

The current `Agent` system should not simply be deleted.

Existing:

```text
TOML agents
Lua agents
Rust agents
```

primarily represent **prompt resolution**.

The new architecture separates:

```text
AgentDefinition
    │
    ▼
PromptResolver
    │
    ▼
AgentRuntime
```

A prompt may therefore still be:

```text
static TOML
Lua-generated
Rust-generated
```

but the runtime now has the option to execute the resulting agent itself.

Existing MCP prompt behavior remains supported.

---

# 7. ADR-0014

ADR-0014 currently establishes agents as stateless prompt generators and explicitly delegates execution to external clients.

This design does not silently invalidate that ADR.

A new ADR should establish:

> Context Harness supports both stateless prompt projection and runtime-owned agent execution.

ADR-0014 should remain historically accurate for the MCP prompt model but be superseded as the complete definition of a Context Harness agent.

The distinction becomes:

```text
Prompt mode
    external client owns state

Runtime mode
    Context Harness owns execution state
```

---

# 8. Model Runtime

Introduce a provider-neutral model abstraction.

Conceptually:

```rust
trait ModelProvider {
    async fn generate(
        &self,
        request: ModelRequest,
    ) -> Result<ModelResponse>;
}
```

Core runtime types should represent:

```text
messages
tool definitions
tool calls
usage
finish reason
structured output
streaming events
```

without exposing provider SDK types.

Initial providers:

```text
FakeModel
one production provider
```

Additional providers can be added independently.

Model configuration should be declarative:

```toml
[models.reasoning]
provider = "openai"
model = "..."

api_key_env = "OPENAI_API_KEY"
```

Agents reference:

```toml
model = "reasoning"
```

---

# 9. Agent Runtime

Introduce an `AgentRuntime` responsible for executing one resolved agent.

Conceptually:

```text
input
  ↓
resolve agent
  ↓
resolve model
  ↓
resolve effective tool set
  ↓
construct context
  ↓
model request
  ↓
┌──────────── final output ────────────┐
│                                      │
└─ tool request                         │
       ↓                               │
  permission check                     │
       ↓                               │
  execute tool                         │
       ↓                               │
  append result                        │
       └──── model request again        │
                                       ▼
                                  run complete
```

Termination conditions:

```text
final model response
max turns
timeout
model failure
fatal tool failure
permission rejection
explicit cancellation
```

---

# 10. SQLite Remains the Runtime Backbone

Do not introduce Postgres, Kafka, Redpanda, or an object store.

Extend the existing SQLite database.

Add tables conceptually equivalent to:

```text
agent_runs
agent_events
tool_invocations
agent_checkpoints
```

## `agent_runs`

Tracks the current materialized state of a run.

```text
id
workspace_id
agent_name
agent_version
model
status
created_at
updated_at
completed_at
input
output
error
```

## `agent_events`

Append-only execution log.

```text
sequence
run_id
timestamp
event_type
payload
```

Example events:

```text
run.started
context.resolved

model.requested
model.responded

tool.requested
tool.started
tool.completed
tool.failed

approval.requested
approval.granted
approval.denied

agent.delegated

checkpoint.created

run.completed
run.failed
run.cancelled
```

This becomes the canonical history of what occurred.

---

# 11. Checkpointing and Resume

Agent runs should be resumable.

A checkpoint contains enough resolved execution state to continue:

```text
agent resource version
model
conversation messages
tool state references
turn number
workspace
permission context
```

CLI:

```bash
ctx agent resume <run-id>
```

The system should not depend exclusively on reconstructing a large conversation from every historical event.

Use:

```text
append-only events
        +
periodic state checkpoints
```

Events provide history.

Checkpoints provide efficient resume.

---

# 12. Artifacts

Large run outputs should live on the local filesystem.

Example:

```text
.ctx/
└── runs/
    └── <run-id>/
        └── artifacts/
```

SQLite stores artifact metadata.

Artifacts may include:

```text
patches
large model responses
test output
generated documents
research output
```

No external object store is required.

---

# 13. Tool Runtime

Reuse the existing `Tool` and `ToolRegistry` model.

The runtime should not create a second tool abstraction unless necessary.

Existing tools become directly callable by agents.

Conceptually:

```text
AgentRuntime
      │
      ▼
ToolRegistry
      │
      ├── search
      ├── get
      ├── Lua tool
      ├── Rust tool
      └── MCP tool
```

This is one of the strongest reasons to evolve Context Harness rather than start over.

---

# 14. Developer Tools

Context Harness currently emphasizes context tools.

The developer harness requires workspace capabilities.

Introduce a small core set such as:

```text
workspace.read
workspace.search

git.status
git.diff

process.exec

workspace.patch
```

These capabilities must be governed by permissions.

Do not put behavioral rules such as:

> never modify files

only in prompts.

The runtime must enforce them.

---

# 15. Permission Model

Every tool should expose capability metadata.

At minimum:

```text
read_only
workspace_write
process_execute
network
external_side_effect
```

An effective agent permission set is:

```text
registered tools
      ∩
agent-declared tools
      ∩
policy permissions
```

Example:

```toml
[agent.permissions]
allow = [
  "read_only",
  "process_execute",
]

require_approval = [
  "workspace_write",
]
```

The runtime, not the model, performs authorization.

---

# 16. Approval Flow

Interactive execution should support approval.

Example:

```text
Agent wants:

workspace.patch
  src/foo.rs

Allow?
[y] once
[a] for this run
[n] deny
```

Approval is persisted as events.

Non-interactive mode must have explicit behavior:

```text
deny
allow from policy
fail
```

Never silently approve privileged operations.

---

# 17. MCP Client Support

Context Harness currently acts as an MCP **server**.

To become a general developer harness, it should also become an MCP **client**.

This allows an agent to consume external tool providers:

```text
ctx
 │
 ├── built-in tools
 ├── Lua tools
 └── MCP clients
        │
        ├── GitHub
        ├── browser
        ├── database
        └── custom local provider
```

Configuration:

```toml
[mcp_servers.example]
command = "example-mcp-server"
args = []
```

Discovered MCP tools are adapted into the existing `ToolRegistry`.

This should be treated as another tool source, not as a separate agent architecture.

---

# 18. Context as a First-Class Runtime Capability

Context Harness's primary differentiator should remain its context engine.

Agents should naturally have access to:

```text
search
get
sources
workspace context
```

An agent may explicitly call those tools.

Additionally, an agent may declare initial context behavior:

```toml
[agent.context]
mode = "dynamic"
max_results = 8
```

The runtime may use existing prompt-resolution mechanisms to inject initial project context.

Do not force all retrieval into automatic RAG.

Explicit tool-driven retrieval remains first-class.

---

# 19. CLI

Add an `agent` command family.

```bash
ctx agent list

ctx agent show reviewer

ctx agent validate

ctx agent run reviewer \
  "Review my current changes"

ctx agent resume <run-id>

ctx agent history

ctx agent inspect <run-id>
```

Useful development options:

```bash
ctx agent run reviewer \
  "Review this change" \
  --json
```

and:

```bash
ctx agent run implementer \
  "Implement issue 123" \
  --non-interactive
```

---

# 20. Direct Invocation Is the Primary Runtime

Unlike the distributed agent-platform design, no broker is required.

Execution begins directly:

```text
CLI
 ↓
AgentRuntime
 ↓
Agent
```

The runtime may use Tokio channels internally, but these are implementation details.

There is no need to emulate Kafka locally.

---

# 21. Local Task Queue

If asynchronous local execution becomes useful, use SQLite.

Conceptually:

```text
agent_tasks
```

with:

```text
id
agent
input
status
created_at
claimed_at
completed_at
```

Workers may claim tasks transactionally.

Do not introduce a message broker for a single-user developer tool.

This feature is optional and should follow direct execution rather than precede it.

---

# 22. Agent Delegation

Multi-agent behavior should initially be implemented as explicit delegation, not as a workflow engine.

An agent may declare:

```toml
[agent.delegation]
allow = [
  "reviewer",
  "researcher",
]
```

The runtime exposes a controlled internal capability equivalent to:

```text
agent.invoke
```

Example:

```text
Planner
   │
   ├── delegate → Researcher
   │
   └── delegate → Reviewer
```

Each delegated execution receives its own run identity while retaining:

```text
parent_run_id
root_run_id
```

This provides traceability without introducing a DAG language.

---

# 23. Agent Composition Example

```text
User
 │
 ▼
Implementer
 │
 ├── search project context
 │
 ├── inspect files
 │
 ├── modify files
 │
 ├── run tests
 │
 └── delegate
        │
        ▼
      Reviewer
        │
        ├── inspect diff
        ├── search conventions
        └── return findings
 │
 ▼
Implementer
 │
 ▼
final response
```

Everything occurs within:

```text
one ctx binary
one SQLite database
one local workspace
```

except LLM calls and explicitly configured external tools.

---

# 24. Workspace Integration

Reuse the existing workspace model.

Runs must bind to a workspace at creation.

The runtime uses that workspace to resolve:

```text
configuration
context database
agent resources
tool resources
filesystem root
```

A run cannot silently move to another workspace.

Workspace-scoped extension behavior from DESIGN-0009 should also apply to executable agents.

---

# 25. Resource Resolution

Effective agent resources follow:

```text
global agents
      ↓ overridden by
workspace agents
```

Effective tools follow the existing extension model.

A workspace should therefore be self-contained:

```text
project/
├── .ctx/
│   ├── config.toml
│   ├── agents/
│   │   ├── reviewer.toml
│   │   └── implementer.toml
│   └── policies/
│
└── source/
```

A developer cloning the project receives its agent definitions with the repository.

---

# 26. MCP Prompt Compatibility

Every executable agent should still be projectable as an MCP prompt where practical.

```text
AgentDefinition
       │
       ├──── ctx agent run
       │
       └──── MCP prompts/get
```

MCP clients therefore continue to benefit from the same agent definitions.

The difference is simply who owns execution.

---

# 27. Event-Driven Internals Without Distributed Infrastructure

The runtime should internally think in events:

```text
run.started
model.responded
tool.completed
run.completed
```

but events remain local SQLite records.

This preserves many of the useful properties discussed for distributed systems:

```text
auditability
debuggability
replay
resume
causality
metrics
```

without adopting distributed-system infrastructure.

---

# 28. Observability

Every run should be inspectable.

Example:

```bash
ctx agent inspect 01K...
```

Output:

```text
Run: 01K...
Agent: implementer
Model: ...
Workspace: context-harness

00:00.000 run.started
00:00.041 context.resolved
00:00.052 model.requested
00:02.918 model.responded
00:02.920 tool.requested    workspace.read
00:02.924 tool.completed
00:02.925 model.requested
...
00:36.122 run.completed
```

This should become a major debugging feature.

---

# 29. Repository Structure

Keep the existing workspace.

A reasonable evolution is:

```text
crates/
├── context-harness-core/
│
├── context-harness-agent/
│   ├── model/
│   ├── runtime/
│   ├── resources/
│   ├── events/
│   ├── policy/
│   └── checkpoint/
│
└── context-harness/
    ├── CLI
    ├── connectors
    ├── MCP server
    ├── tools
    └── workspace integration
```

`context-harness-agent` is an internal implementation crate.

The product remains:

```text
ctx
```

This split is optional during early implementation, but is the preferred long-term boundary if runtime code grows substantially.

---

# 30. Non-Goals

Do not add:

```text
Postgres

Kafka / Redpanda

MinIO / S3 requirement

Kubernetes

distributed workers

multi-user scheduling

cloud control plane

workflow DAG engine

Temporal replacement

vector database requirement

web administration UI
```

Context Harness remains fundamentally local-first developer tooling.

---

# 31. Implementation Plan

## Phase 1 — Execution Persistence

Extend SQLite with:

```text
agent_runs
agent_events
tool_invocations
agent_checkpoints
```

Implement event append/read APIs.

Acceptance:

```text
a synthetic run can be created,
events appended,
state inspected,
and a checkpoint persisted
```

No LLM required.

---

## Phase 2 — Declarative Agent Resources

Introduce standalone agent resources.

Implement:

```text
loading
workspace/global resolution
validation
model reference
tool declarations
permissions
execution limits
```

Add:

```bash
ctx agent list
ctx agent show
ctx agent validate
```

Acceptance:

Existing and new agent definitions can coexist.

---

## Phase 3 — Model Runtime

Implement:

```text
ModelProvider
ModelRegistry
FakeModel
first real provider
```

Persist model request/response lifecycle events.

Acceptance:

A model can be invoked outside the agent loop.

---

## Phase 4 — Local Agent Loop

Implement `AgentRuntime`.

Add:

```bash
ctx agent run
```

Initially support:

```text
model
built-in search/get tools
static prompts
run history
```

Acceptance:

```text
ctx agent run researcher "..."
```

can perform multiple tool/model turns and produce a final answer.

This is the primary architectural milestone.

---

## Phase 5 — Developer Tools and Policies

Add:

```text
workspace.read
workspace.search
git.status
git.diff
process.exec
workspace.patch
```

Implement capability metadata and approval enforcement.

Acceptance:

A read-only reviewer cannot write even if the model requests it.

---

## Phase 6 — Checkpoint and Resume

Implement:

```bash
ctx agent resume <run-id>
```

Test interruption during:

```text
model execution
tool execution
between turns
```

Resume from the most recent valid checkpoint.

---

## Phase 7 — MCP Client Tool Providers

Add MCP client capability.

External MCP tools enter the existing `ToolRegistry`.

Acceptance:

A locally running agent can invoke an external MCP tool without special-case agent code.

---

## Phase 8 — Delegation

Add controlled:

```text
agent.invoke
```

Implement:

```text
parent_run_id
root_run_id
delegation permission
```

Acceptance:

One declarative agent can delegate a bounded task to another declarative agent.

---

## Phase 9 — MCP Projection

Ensure executable agents remain usable through existing MCP prompt interfaces.

The existing Context Harness MCP use case must remain intact.

---

# 32. First Target Experience

Repository:

```text
my-project/
├── .ctx/
│   ├── agents/
│   │   ├── implementer.toml
│   │   └── reviewer.toml
│   └── config.toml
└── src/
```

Then:

```bash
ctx sync all

ctx agent run implementer \
  "Add validation to the configuration loader"
```

Context Harness:

```text
loads project context

loads implementer agent

resolves model

resolves permitted tools

calls model

searches indexed project knowledge

reads source files

modifies allowed files

runs tests

delegates review if requested

persists every execution event

returns final result
```

Later:

```bash
ctx agent inspect <run-id>
```

shows exactly how it got there.

---

# 33. Definition of Done

The transformation is successful when:

- `ctx` remains a single local binary;
- existing ingestion/retrieval remains intact;
- existing MCP server behavior remains intact;
- agents can be declared without Rust code;
- `ctx agent run` executes an agent itself;
- the runtime owns model/tool iteration;
- all runs have durable SQLite history;
- interrupted runs can resume;
- tool permissions are enforced outside prompts;
- local developer tools are available;
- external MCP tools can be consumed;
- agents can explicitly delegate to other agents;
- no external infrastructure is required;
- only the model provider and explicitly configured external services require network access;
- Context Harness's existing context engine is a first-class part of agent execution.

---

# 34. Final Architecture

```text
                         ctx

              ┌───────────────────────┐
              │   Agent Definition    │
              └───────────┬───────────┘
                          │
                          ▼
                 ┌─────────────────┐
                 │  Agent Runtime  │
                 └────────┬────────┘
                          │
       ┌──────────────────┼───────────────────┐
       │                  │                   │
       ▼                  ▼                   ▼
 Context Engine       Model Runtime        Tool Registry
       │                  │                   │
       ▼                  ▼          ┌────────┼──────────┐
 SQLite / FTS       LLM Provider      ▼        ▼          ▼
 embeddings                        built-in   Lua        MCP
                                      │
                                      ▼
                                  workspace
                                      │
                                      ▼
                               files / git / tests

                          │
                          ▼
                    SQLite Run Log
                          │
                          ▼
                 history / resume / audit
```

Context Harness started as the system that prepares context for an agent.

The proposed evolution makes it the system that can **run the agent using that context**.

External harnesses remain supported.

They are no longer required.