+++
title = "v0.9.0: TODO_TITLE"
description = "TODO_DESCRIPTION"
date = 2026-10-14

[taxonomies]
tags = ["release"]
+++

<!-- Auto-generated release blog post for v0.9.0 -->
<!-- Range: v0.8.0..v0.9.0 -->
<!-- Edit this file, then commit and push to deploy -->

Context Harness v0.9.0 is out.

### What changed

<!-- Commits since last release:

8ee6141 Merge pull request #66 from parallax-labs/codex/release-0-9-0
548fa16 chore: bump workspace version to 0.9.0
366f714 Merge pull request #65 from parallax-labs/codex/phase-6d-site-refresh
4215652 docs: graduate agent foundation and refresh site
1ddaac5 Merge pull request #64 from parallax-labs/codex/context-refresh-baselines
76daa00 perf(agent): complete compatibility baselines
318e050 Merge pull request #63 from parallax-labs/codex/compatibility-performance-baselines
f2ef25a perf(agent): establish initial foundation baselines
d47df79 Merge pull request #62 from parallax-labs/codex/integrated-failure-matrix
b3db2fd test: complete integrated failure matrix
0e95a37 Merge pull request #61 from parallax-labs/codex/cross-track-fixture
92d9f63 test(agent): prove cross-track host composition
f4e2768 Merge pull request #60 from parallax-labs/codex/suspended-run-continuation-implementation
30181db feat(agent): continue suspended runs
4cd9665 Merge pull request #59 from parallax-labs/codex/suspended-run-continuation-design
c51999a docs: define suspended run continuation
5d22648 Merge pull request #58 from parallax-labs/codex/recoverable-tool-observations-implementation
e8dae10 feat(agent): add recoverable tool observations
3d48228 Merge pull request #57 from parallax-labs/codex/recoverable-tool-observations
7adb998 docs(agent): specify recoverable tool observations
67d1c7c Merge pull request #56 from parallax-labs/codex/bounded-context-projection
75c000c feat(agent): add bounded context projection
df27e82 Merge pull request #55 from parallax-labs/codex/bounded-context-projection-design
de2c3d5 docs(agent): define bounded context projection
48c3002 Merge pull request #54 from parallax-labs/codex/working-state-implementation
ae17f7e feat(agent): materialize structured working state
a86719c Merge pull request #53 from parallax-labs/codex/structured-working-state
3de3c69 docs(agent): define structured working state
291681b Merge pull request #52 from parallax-labs/codex/agent-task-cli
59ad879 feat(agent): add durable task CLI
bd632b6 Merge pull request #51 from parallax-labs/codex/agent-task-reconciliation
8067bf2 feat(agent): reconcile durable tasks
72de86d Merge pull request #50 from parallax-labs/codex/agent-task-worker
b595216 test(agent): await durable worker heartbeats
3970446 feat(agent): add durable task worker
50e60c7 Merge pull request #49 from parallax-labs/codex/agent-task-store
b23b903 feat(agent): add durable task submission store
2c8d096 Merge pull request #48 from parallax-labs/codex/durable-agent-tasks
e7864c2 Specify durable agent task ownership
ddc6968 Merge pull request #47 from parallax-labs/codex/checkpoint-recovery-evolution
340f533 Evolve agent checkpoint recovery metadata
b63d4c5 Merge pull request #46 from parallax-labs/codex/cumulative-run-budgets
80c47f4 feat(agent): persist cumulative run budgets
3b08239 Merge pull request #45 from parallax-labs/codex/typed-run-outcomes-runtime
0154650 Implement typed agent run outcomes
0e51d1e Merge pull request #44 from parallax-labs/codex/typed-run-outcomes
9c471ef docs(agent): define durable run lifecycle contract
30f6f4f Merge pull request #43 from parallax-labs/codex/targeted-refresh-api
51abed4 feat: add targeted document refresh API
40635a8 Merge pull request #42 from parallax-labs/codex/targeted-refresh-contract
3a0309b docs: define targeted refresh contract
ec13eca Merge pull request #41 from parallax-labs/codex/shared-document-ingestor
561c388 refactor(ingest): extract shared document ingestor
8952530 Merge pull request #40 from parallax-labs/codex/ollama-generation-provider
00be828 feat(agent): add local Ollama generation provider
70a3796 Merge pull request #39 from parallax-labs/codex/ollama-provider-contract
2f53b13 docs(agent): define Ollama provider contract
0b4c5ab Merge pull request #38 from parallax-labs/codex/public-agent-host
1578e47 feat(agent): add public agent host builder
e9ab9b8 Merge pull request #37 from parallax-labs/codex/model-provider-catalog
bbc8218 feat(agent): add model provider catalog seam
e85ed3d Merge pull request #36 from parallax-labs/codex/async-local-agent-jobs
e69205b docs: plan local agent application foundation
039290b Merge pull request #35 from parallax-labs/codex/agent-run-lifecycle-context
8920697 Merge remote-tracking branch 'origin/main' into codex/agent-run-lifecycle-context
ccb1aae Merge pull request #34 from parallax-labs/codex/declarative-tool-bindings
93e4a91 docs: draft durable agent run architecture
91b77a0 feat: rename prompt agents to profiles
e878c03 docs: promote agents and profiles
913c20c docs: explain agent setup skill workflow
78b3744 docs: distinguish profiles from executable agents
21c4108 fix: release run ownership lock explicitly
3814e69 docs: add local agent setup and guidance
56f6854 fix: use current atomic update API
be0f896 fix: scope async trait lint compatibility
420d91a fix: accommodate Rust 1.99 async trait lint
7a523fa docs: complete declarative binding rollout
4efb860 fix: redact binding secrets in inspection
46e501a feat: record compatibility binding identities
7843778 refactor: validate compatibility tools through adapters
2fc39ee test: cover scoped reader composition
1bbb583 fix: cancel privileged Lua tool workers
f6d179c feat: bind MCP tool aliases
812205b feat: authorize privileged Lua tool bindings
84639b1 feat: support compiled tool binding catalogs
78d3448 feat: bind tool identities to recovery
029003d feat: bind source-scoped retrieval tools
802eb39 feat: execute scoped tool bindings
d82386a feat: resolve and inspect tool bindings
c2fc11c feat: define declarative tool binding contracts
6208044 Merge pull request #33 from parallax-labs/codex/local-agent-runtime
7a40ccc docs: illustrate declarative tool composition with a release reviewer
7e9b334 docs: prioritize declarative tool foundation and defer wiki design
ed9f7bc docs: link wiki curation product intent and design handoff
7916799 docs: plan local wiki curation through Claude Code hooks and Ollama
bfd506b fix(ci): use fixed-size chunks when decoding embeddings
70bfeae feat(agent): project standalone resources as stateless MCP prompts
9227d52 feat(agent): add bounded delegation with inherited authority
2f21172 feat(agent): add authorized stdio MCP client tools
4d8a02a feat(agent): add conservative checkpoint recovery and output artifacts
97c39ae feat(agent): add developer tools and durable invocation approvals
b995919 feat(agent): execute bounded local runs with durable tool history
1a52ee6 feat(agent): add model registry and OpenAI Responses adapter
19f8086 feat(agent): add standalone resources and validation commands
2340e09 feat(agent): add durable runtime persistence and execution plan
5dc82bd Merge pull request #31 from parallax-labs/feature/multi-workspace-router-phase2
6304aee Merge pull request #32 from parallax-labs/fix/hybrid-search-or-terms
1037146 test(search): cover FTS OR-join for multi-term natural-language queries
afb97b6 fix(search): OR-join FTS query terms so natural-language queries match
bd4e9fc refactor(router): address final-review minors (errors ordering, error context, tests, docs)
70579f7 style(workspace): rustfmt the Phase 2 test block (line-length drift)
16044c7 docs: mark multi-workspace router Phase 2 (all fan-out) as implemented
78a12c4 style(router): rustfmt the workspace=all test block added in the prior commit
36c8a5e test(router): cover workspace=all fan-out for search and sources
0e21e38 feat(router): implement workspace=all fan-out for search and sources
298551a feat(workspace): add bounded fan_out helper with per-item deadline
7583442 feat(workspace): add resolve_all and configurable fan-out deadline
788c3c9 feat(workspace): add workspace_timeout RouterError code
7ca26a9 feat(workspace): add [defaults].search_deadline_ms registry knob
83f9c8e plan: multi-workspace router Phase 2 (all fan-out) implementation plan
7a37c63 design: finalize Phase 2 all-workspace fan-out (DESIGN-0008)
b792b5e Merge pull request #30 from parallax-labs/feature/multi-workspace-router-phase1
54df8b4 feat: multi-workspace MCP router — Phase 1 (built-in routing + security gates)
02a6379 design: multi workspace routing design
a275980 design: added runbook
dfb8cb7 initial codex design
881a0e3 Merge pull request #29 from parallax-labs/release-blog/v0.8.0
a40a803 Fix release blog PR detection
c0e460c Add v0.8.0 release blog post

Files changed:  197 files changed, 44873 insertions(+), 1869 deletions(-)

-->

TODO: Write the release notes here. Group changes into sections:
- New features
- Improvements
- Bug fixes
- Breaking changes (if any)

### Upgrading

```bash
# From source
cargo install --path . --force

# Pre-built binary
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
```

### What's next

TODO: What's coming in the next release.
