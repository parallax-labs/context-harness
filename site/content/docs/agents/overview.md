+++
title = "Agents Overview"
description = "Turn indexed project knowledge into bounded work that can be inspected, governed, and recovered."
weight = 1
+++

Context Harness agents do more than retrieve context. They run a complete,
bounded model-and-tool loop inside the project where the work belongs.

Give an agent a durable role, a model, a deliberately small tool set, and a task:

```sh
ctx agent run code-reviewer \
  "Review this change against our documented error-handling conventions."
```

Context Harness supplies relevant local context, enforces the configured
authority, records what happened, and leaves behind an inspectable run.

### What agents are for

Agents are the right fit when you want Context Harness—not an editor or external
chat client—to own task execution.

| Goal | Example agent | Useful tools |
|---|---|---|
| Understand a project | Project researcher | Source-scoped search and retrieval |
| Review a change | Code reviewer | Project search, document read, Git inspection |
| Prepare a release | Release assistant | Git status, changelog context, approved process execution |
| Investigate a failure | Incident analyst | Runbook search, scoped logs, approved external tools |
| Coordinate specialists | Lead agent | Explicitly allowed child agents |

The role is generic. The project-specific knowledge comes from connectors and
tool bindings, so the same agent design can work across repositories without
hard-coding a wiki, vendor, or business domain.

### The agent stack

```
Project sources
      │
      ▼
Connectors and local index
      │
      ▼
Scoped tool bindings ── permissions and approvals
      │
      ▼
Agent role + task input
      │
      ▼
Bounded model/tool loop
      │
      ▼
Answer, artifacts, and durable run history
```

Each layer has one job:

- **Connectors** decide what knowledge enters the local index.
- **Tool bindings** expose a narrow public capability over a trusted
  implementation.
- **Agent resources** choose the model, tools, role, permissions, and execution
  limits.
- **The runtime** validates calls, obtains approvals, executes tools, and records
  checkpoints and results.

This separation lets you reuse an implementation without giving every agent its
full authority.

### Why run agents here?

**Grounded by default.** Agents can search the same local context you already
index for CLI and MCP retrieval. They can fetch the full source before making a
claim instead of relying on a copied excerpt.

**Small, explicit authority.** An agent sees only its declared public tools.
Fixed arguments and source or path restrictions are enforced below the prompt,
and higher-risk capabilities can require interactive approval.

**Inspectable execution.** Run history records model turns, tool calls,
approvals, usage, artifacts, and terminal state without persisting credentials or
raw provider response bodies.

**Conservative recovery.** Interrupted safe runs can resume from a checkpoint.
Changed definitions, uncertain side effects, and unreconstructable external
sessions stop rather than replaying work blindly.

### An agent is not a permanent chat identity

Conversation state is retained during one run and in recovery-safe checkpoints.
A new run starts fresh with the configured system prompt and new task input.
Previous runs remain available for inspection, but are not automatically added
to future model context.

For durable knowledge across runs today, save the result to a connector source
and sync it. Automatic cross-run memory should be an explicit feature with
scope, retention, provenance, and deletion controls—not silent transcript
replay.

### Agents versus profiles

Use an **agent** when Context Harness should run the model and tools. Use a
**profile** when Cursor, Claude, or your own client already runs the conversation
and only needs a reusable role and context recipe.

| Question | Agent | Profile |
|---|---|---|
| Who calls the model? | Context Harness | External client |
| Who executes tools? | Context Harness runtime | External client |
| Durable runs and inspection? | Yes | No; the client owns its session |
| Permissions and approvals? | Runtime-enforced | Client-enforced |
| Best for | Bounded project work | Consistent external conversations |

See [Profiles Overview](@/docs/profiles/overview.md) when the external client
should remain in charge.

### Define once, choose who executes

A standalone `.ctx/agents/<name>.toml` resource can serve both experiences:

```
.ctx/agents/code-reviewer.toml
              │
       ┌──────┴──────┐
       ▼             ▼
ctx agent run    ctx serve mcp
managed agent    stateless profile
```

Running it locally applies its model, tools, permissions, limits, and durable
run lifecycle. Serving it over MCP projects its system prompt and tool hints for
an external client; it does not transfer runtime enforcement or create history.
That lets a team reuse one role while choosing the right execution model for each
task.

### Start here

Choose one path:

1. Follow [Build Local Agents](@/docs/agents/build-local-agents.md) to understand
   and configure every resource manually.
2. Use the [Agent Setup Skill](@/docs/agents/setup-skill.md) to have Codex inspect
   an existing project and create the smallest safe configuration.
3. Start from the repository's `examples/local-agents` directory when you want
   complete files to compare or copy.

A successful first run is intentionally small: one read-only agent, one indexed
source, source-scoped search and retrieval, static validation, a direct search
smoke test, and then one cited question.
