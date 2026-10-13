+++
title = "Embed an Agent Host"
description = "Compose providers, scoped tools, durable tasks, workers, runs, and targeted refresh through supported public Rust APIs."
weight = 4
+++

Context Harness can be embedded as the execution foundation for a local application
without moving that application's domain policy into the framework. The host owns
its events, files, publication rules, and user experience. Context Harness owns the
generic contracts for model/tool execution, durable tasks and runs, recovery, and
retrieval refresh.

### The boundary

```
Your application
  events · domain state · files · publication · UI
             │
             ▼
Context Harness public APIs
  AgentHostBuilder · provider/tool catalogs
  task worker · TargetedRefresher
             │
             ▼
Canonical local state
  SQLite documents · runs · tasks · events · checkpoints
```

Configuration never loads provider or tool code dynamically. A trusted compiled
host registers implementations explicitly, supplies scoped authority, and then
builds the runtime. Static resource validation remains inert: it does not resolve
secrets, open run databases, contact providers, execute tools, evaluate Lua, start
MCP clients, or spawn processes.

### Public contracts

Build against the authoritative contracts rather than the historical design plans:

- [Model Runtime](https://github.com/parallax-labs/context-harness/blob/main/docs/spec/0016-model-runtime.md)
  defines provider registration, identity, capabilities, errors, and offline inspection.
- [Local Agent Execution](https://github.com/parallax-labs/context-harness/blob/main/docs/spec/0017-local-agent-execution.md)
  defines the bounded model/tool loop and policy boundary.
- [Checkpoints, Recovery and Artifacts](https://github.com/parallax-labs/context-harness/blob/main/docs/spec/0019-checkpoints-recovery-and-artifacts.md)
  defines conservative recovery and durable evidence.
- [Durable Agent Tasks](https://github.com/parallax-labs/context-harness/blob/main/docs/spec/0026-durable-agent-tasks.md)
  defines submission, identity, claims, leases, workers, cancellation, and reconciliation.
- [Targeted Document Refresh](https://github.com/parallax-labs/context-harness/blob/main/docs/spec/0024-targeted-document-refresh.md)
  defines bounded refresh of host-known changed documents without connector discovery.
- [Suspended Run Continuation](https://github.com/parallax-labs/context-harness/blob/main/docs/spec/0030-suspended-run-continuation.md)
  defines explicit, same-run continuation after blocked or needs-input outcomes.

The integration fixture in `crates/context-harness/tests/agent_host.rs` is the
executable reference: it registers a compiled provider and scoped tool, submits and
runs a durable task, observes typed task/run outcomes, writes a host-owned document,
refreshes it, and verifies retrieval using public APIs only.

### Operational handoff

1. Start with [Build Local Agents](@/docs/agents/build-local-agents.md) for resource,
   model, tool, permission, and validation concepts.
2. Run workers in the foreground or under your own service manager using
   [Operate a Durable Agent Task Worker](https://github.com/parallax-labs/context-harness/blob/main/docs/runbook/0020-operate-agent-task-worker.md).
3. Establish host-specific baselines with
   [Measure Agent Foundation Performance](https://github.com/parallax-labs/context-harness/blob/main/docs/runbook/0021-agent-foundation-performance.md).
4. Keep application job IDs correlated with Context Harness request keys, task IDs,
   and run IDs; do not duplicate the run lifecycle in application state.

Context Harness intentionally does not define hooks, wiki semantics, evidence rules,
publication formats, or application lifecycle states. Those remain in the host.
