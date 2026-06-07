# DESIGN-0008: Graph Retrieval Benchmark Protocol

**Status:** Draft
**Date:** 2026-06-07
**Author:** Parker Jones
**Related:** [SPEC-0003](../spec/0003-hybrid-scoring.md), [SPEC-0008](../spec/0008-lua-connectors.md), [SPEC-0009](../spec/0009-lua-tools.md), [SPEC-0011](../spec/0011-mcp-agents.md)
**Source:** ChatGPT conversation export, "context harness graph - Context Routing for KG" (`/Users/pjones/Documents/notes/+ Sources/context harness graph - Context Routing for KG.md`)

## Context

Context Harness already provides connector-driven ingestion, chunking, SQLite/FTS storage, embeddings, hybrid retrieval, CLI search, MCP tools, Lua tools, and agent prompt injection. That makes it a better foundation for graph-assisted context routing than starting a separate knowledge graph product.

The project question is deliberately narrower than "can we build a knowledge graph":

> Can graph edges reduce the amount of context an AI coding agent needs to read, without reducing answer quality?

The initial motivation is to support Claude Code and Codex workflows with compact, reliable project context. The personal/public side of the work should remain generic to Context Harness and public repositories. Work-specific corpora, internal tickets, logs, customer data, proprietary code, and credentials must stay in work-approved tooling and local private configuration.

The working split from the source conversation is:

- **Personal/public work:** Codex, ChatGPT, GitHub public repositories, Context Harness public docs, generic benchmark framework, generic graph schema, public examples.
- **Rithum/work work:** Claude Code in a work-approved environment, DSLA task corpora, Jira/PR/runbook connectors, internal repo indexing, real work benchmark results.

No work task, ticket, log, stack trace, customer name, internal URL, proprietary code, or credential should be ingested into ChatGPT/Codex-oriented context.

## Proposal

Add graph retrieval capability only after a benchmark proves it improves context routing over existing hybrid search.

The first phase should produce a benchmark protocol and task corpus. Graph storage and MCP tools should follow only when the baseline is measurable.

### Benchmark modes

Compare three modes for the same task corpus:

1. **Baseline/manual:** the agent uses ordinary repo search and file reads.
2. **Hybrid retrieval:** the agent uses existing Context Harness keyword/vector retrieval.
3. **Graph retrieval:** the agent uses Context Harness retrieval plus graph edges.

For each task, record:

- prompt
- retrieved files and documents
- total input/context tokens
- useful-context precision
- missed required context
- answer quality
- time-to-useful-answer
- whether the agent hallucinated, stalled, or needed repeated rediscovery

### Success criteria

Continue graph work only if graph retrieval shows:

- 30-50% fewer context tokens on at least 6 of 10 benchmark tasks.
- Equal or better answer quality than hybrid retrieval.
- Explainable retrieval results.
- Sync/index maintenance that is not more expensive than the savings.

### Kill criteria

Pause or kill the graph layer if:

- Hybrid search performs almost as well.
- Graph extraction is fragile or noisy.
- Maintaining entities takes more time than it saves.
- Agents still need to read the same full files for most tasks.
- The graph becomes a second product instead of a compact context-routing layer.

### Minimal graph model

Start inside the existing SQLite database, not SurrealDB.

Initial tables:

```sql
graph_nodes
graph_edges
```

Initial node types:

- file
- symbol
- module
- document
- concept

Initial edge types:

- `file -> defines -> symbol`
- `file -> imports -> file`
- `document -> explains -> file`
- `document -> explains -> concept`
- `symbol -> belongs_to -> module`

Defer call graphs, LLM-generated relationships, ticket graphs, PR graphs, and external graph databases until the benchmark shows that graph proximity helps.

### Initial graph-aware tools

Expose at most three graph-aware MCP/tool surfaces first:

- `context.related`
- `context.explain_module`
- `context.trace_symbol`

These tools should return compact context with source IDs and enough explanation for an agent to understand why each item was selected.

### Initial self-hosted benchmark tasks

Use Context Harness itself as the first task corpus:

1. Trace hybrid scoring implementation.
2. Explain MCP request flow.
3. Find where `ToolContext` is exposed.
4. Explain how agents inject context.
5. Trace ingestion pipeline end-to-end.
6. Find embedding staleness logic.
7. Explain registry resolution.
8. Determine files needed to add a new MCP tool.
9. Find public API schema definitions.
10. Determine where a graph provider should plug into the architecture.

These tasks are intentionally self-hosted, public, and grounded in the repo. Work-specific tasks can be evaluated separately in a private work environment.

### Codex/GitHub handoff

The practical workflow is:

```text
ChatGPT conversation
  -> distilled project spec / issue / PR plan
  -> Codex gets the repo + task
  -> Codex creates branch, commit, and PR
```

The first PR should be documentation-only:

```text
Branch: benchmark-first graph retrieval experiment
PR title: docs: add graph benchmark protocol
Scope: add the benchmark protocol and planning artifact only; do not implement graph code.
Rationale: intentionally benchmark-first to avoid building a knowledge graph before proving token savings.
```

## Alternatives Considered

### Start with SurrealDB

SurrealDB is plausible for a future graph/vector store, but it should not be the first implementation. Context Harness already uses SQLite for local-first storage, and the first question is whether graph edges improve retrieval at all. Adding a new database before proving that would increase complexity too early.

### Build a complete code knowledge graph

Not chosen for the first slice. A complete graph of symbols, calls, references, tickets, pull requests, incidents, and documents risks becoming a separate product. Agents need a small number of reliable facts more than they need a perfect graph.

### Use work tasks as the first benchmark corpus

Not chosen for the public project. Work tasks are valuable for real-world validation, but they belong in a private configuration and work-approved environment. The public benchmark should use Context Harness tasks and public repositories only.

## Implementation Plan

1. **Create benchmark task artifacts**
   - Add `benchmarks/tasks/ctx-graph-001.json` through `ctx-graph-010.json`.
   - Include prompt, expected files, expected concepts, and scoring notes.

2. **Add benchmark runner**
   - Run each task in baseline and hybrid modes first.
   - Capture retrieved context, token counts, and result metadata.
   - Store outputs under `benchmarks/results/`.

3. **Add SQLite graph schema**
   - Add migrations for `graph_nodes` and `graph_edges`.
   - Keep graph tables optional and non-invasive.

4. **Add code/document entity extraction**
   - Extract file, symbol, module, document, and concept nodes.
   - Emit only the initial edge set listed above.

5. **Add graph retrieval scoring**
   - Combine hybrid scores with graph proximity.
   - Make graph contribution explainable in `--explain` output.

6. **Expose graph-aware tools**
   - Add `context.related`, `context.explain_module`, and `context.trace_symbol`.
   - Keep outputs compact and source-linked.

7. **Evaluate go/no-go**
   - Run all 10 tasks across baseline, hybrid, and graph modes.
   - Continue only if success criteria are met.

## Open Questions

1. Should benchmark token counts be measured with a specific model tokenizer, or estimated from character/token heuristics until a provider abstraction exists?
2. Should graph retrieval be a new search mode (`--mode graph`) or a hybrid scoring option once implemented?
3. Should benchmark tasks live in this repository permanently, or be generated from a documented task schema?
4. What quality rubric should be used for answer scoring: human review only, deterministic expected-context checks, or both?
