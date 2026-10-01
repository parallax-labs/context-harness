+++
title = "Local Agents, Durable Runs, and Declarative Tools"
description = "Context Harness now runs project agents locally with scoped tool bindings, approvals, recovery, delegation, and extension backends that remain inspectable before execution."
date = 2026-10-01

[taxonomies]
tags = ["agents", "tools", "local-first"]
+++

Context Harness started with a focused promise: put useful project context behind
a small, local, inspectable retrieval layer. The latest work takes the next step.
Context Harness can now run agents locally, persist their execution, and give them
purpose-built tools without turning a TOML file into an unchecked plugin system.

The result is a practical agent runtime for real projects:

- standalone agent definitions that live with the workspace;
- provider-neutral model boundaries with an OpenAI Responses adapter;
- bounded model/tool loops with explicit permissions and approvals;
- durable history, checkpoints, cancellation, and safe recovery;
- delegation with inherited authority and shared budgets;
- declarative public tools backed by trusted built-in, compiled Rust, privileged
  Lua, or MCP implementations;
- static inspection that does not read secrets or start executable backends.

It is enough machinery to build useful project agents, while keeping the local
security boundary visible.

### An agent is now a project resource

A standalone agent is a small TOML file in `.ctx/agents`:

```toml
[agent]
name = "project-researcher"
description = "Answers project questions from indexed evidence"
model = "default"
tools = ["project.search", "project.get"]

[agent.execution]
max_turns = 8
timeout_seconds = 180

[agent.permissions]
mode = "read-only"

[prompt]
system = """
Search project context before answering factual questions. Retrieve full
documents when excerpts are not enough, cite the files you use, and identify
missing evidence instead of guessing.
"""
```

The immediate request stays outside the resource:

```sh
ctx agent run project-researcher \
  "Explain the architecture and cite the files you used."
```

That separation matters. The resource defines a reusable role, limits, and
authority. The run input defines today's job. The same pattern works for code
review, release planning, incident analysis, documentation work, and many other
project workflows. It is not tied to a particular wiki or source system.

### Tool names no longer have to equal implementations

Agents should receive tools named for the job they need to do. Hosts, meanwhile,
need to know exactly which implementation will run and which authority it can
exercise. Declarative bindings connect those two views.

```toml
schema_version = 1

[tool]
name = "project.search"
implementation = "builtin.retrieval.search"
description = "Search indexed project files"

[config]
source = "filesystem:project"

[fixed]
limit = 8

[restrictions]
sources = ["filesystem:project"]
max_output_bytes = 65536
```

The model sees a small `project.search` schema. It cannot change the fixed result
limit or search another source. At runtime, Context Harness validates the public
arguments, adds the fixed values, checks the effective arguments, applies the
source accessor, bounds the result, and records the resolved binding identity.

This is composition, not dynamic code loading. A resource can select and narrow a
trusted implementation. It cannot download a plugin, grant itself a capability,
or escape the host's authority.

### Static inspection is a real boundary

Agent systems are much easier to operate when “show me the configuration” does
not secretly mean “start everything.” The new inspection commands are deliberately
side-effect-free:

```sh
ctx agent validate
ctx agent show project-researcher --json
ctx tool bindings validate
ctx tool bindings show project.search --json
```

They do not open the context database, resolve credential values, evaluate Lua,
spawn MCP processes, access the network, or call a model. Secret-bearing fields
retain environment-variable references and are redacted from inspection and
durable metadata.

MCP aliases make the distinction especially clear. Static validation can prove
that a local declaration is well-formed, but it reports the remote metadata as
unresolved. Only separately authorized startup may discover the remote schema.
If that schema no longer matches the binding, preparation fails before the alias
is advertised to the model.

### One lifecycle for every selected tool

The runtime does not grow a new execution branch for every public tool name.
Selected adapters share one lifecycle:

1. resolve the resource and implementation identity;
2. intersect host authority, implementation needs, binding restrictions, agent
   permissions, and runtime policy;
3. validate public and effective arguments;
4. persist approval before any side effect;
5. execute with cancellation and output bounds;
6. persist the result and binding snapshot before model continuation.

That common path now covers source-scoped retrieval, path-scoped file reads,
registered compiled tools, privileged Lua adapters, MCP aliases, developer tools,
and agent delegation.

The backend-specific rules remain explicit. Compiled Rust implementations must be
registered by the embedding host. Privileged Lua requires authorization before
module evaluation, and CPU-bound execution is cooperatively cancelled at VM
instruction boundaries. MCP keeps its process and external-side-effect approval
requirements, and MCP-backed runs are non-resumable.

### Runs survive more than a terminal session

Local execution writes run state, ordered events, tool invocations, approvals,
usage, artifacts, checkpoints, and binding snapshots to SQLite. Operators can
inspect that state without contacting the original provider:

```sh
ctx agent history --limit 20
ctx agent inspect <run-id>
ctx agent resume <run-id>
```

Recovery is intentionally strict. A changed prompt, model binding, tool schema,
fixed argument, restriction, implementation version, or workspace identity can
invalidate resume. A tool invocation with uncertain side effects is never replayed
just because a process restarted. This favors an explainable stop over a
convenient duplicate action.

Delegation follows the same philosophy. A parent can invoke only named child
agents, children cannot gain capabilities denied to the parent, and the tree
shares depth, deadline, and model-turn budgets.

### A generic starting point

The repository now includes two ways to get started:

- `examples/local-agents` contains a project researcher, a code reviewer,
  source-scoped retrieval, and a path-scoped documentation reader.
- `skills/context-harness-agents` is a reusable Codex skill that can inspect an
  existing project, configure model and connector resources, create narrowly
  scoped tools, validate the result, and guide agent implementation. Its bundled
  initializer refuses to overwrite existing files.

The complete walkthrough is in
[Build Local Agents](@/docs/agents/build-local-agents.md), with every new command in the
[CLI reference](@/docs/reference/cli.md). For assisted setup, the
[Agent Setup Skill guide](@/docs/agents/setup-skill.md) documents
installation, invocation, customization, and validation end to end.

### Where this leaves Context Harness

Context Harness is still local-first context infrastructure. SQLite remains the
canonical record, retrieval remains useful on its own, and accelerated indexes
remain rebuildable sidecars. The agent runtime builds on that foundation instead
of replacing it.

What changed is the distance between context and action. You can now define a
project agent, give it a deliberately small tool surface, see exactly what those
tools resolve to, run it with bounded authority, and recover its history after the
terminal is gone.

That is a much stronger base for agents people can actually understand and
operate—not just demos that happen to call tools.
