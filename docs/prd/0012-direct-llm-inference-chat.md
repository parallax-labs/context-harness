# PRD-0012: Direct LLM Inference Chat

**Status:** Planned
**Date:** 2026-03-03
**Author:** Parker Jones

## Problem Statement

Today, users can query Context Harness data via `search` and integrate with LLM clients through MCP, but they cannot chat directly from the `ctx` binary. This creates unnecessary friction for users who want quick, local terminal-based chat workflows without setting up Cursor, Claude Desktop, or a custom MCP client.

## Target Users

- Individual developers using terminal-first workflows.
- Documentation and platform engineers validating indexed context quickly.
- Teams that need a low-overhead fallback when MCP clients are unavailable.

## Goals

1. Enable users to run `ctx chat` for single-turn and interactive chat workflows.
2. Support retrieval-grounded chat using the existing indexed knowledge base.
3. Support ungrounded chat for general model interactions.
4. Provide streaming responses with clear operational errors and reproducible transcripts.
5. Keep existing MCP and agent workflows fully backward-compatible.

## Non-Goals

- Building a multi-user hosted chat service.
- Replacing MCP prompt/tool workflows in external clients.
- Adding GUI chat surfaces in this phase.
- Introducing autonomous tool-calling loops or agent orchestration in `ctx chat` initial release.

## User Stories

1. As a developer, I run `ctx chat "what changed in our deployment flow?"` and get a grounded answer from indexed docs.
2. As an operator, I use interactive mode (`ctx chat`) to ask follow-up questions in one session and save a transcript for incident notes.
3. As a team lead, I force provider/model overrides via flags for reproducible test runs.
4. As a CI/operator workflow owner, I use `--json` mode to consume chat output in scripts.

## Requirements

1. The CLI must provide a `chat` command supporting both single-turn and interactive session modes.
2. Chat mode must support grounded and ungrounded inference.
3. Grounded mode must reuse existing retrieval/index infrastructure.
4. Provider/model settings must be configurable and overridable at runtime.
5. Streaming output must be the default behavior.
6. Session save/load must be supported for reproducibility and debugging.
7. Error output must be categorized and actionable.
8. Existing commands and MCP server behavior must remain unchanged.

## Success Criteria

- A user can complete a full chat session without external MCP tooling.
- Grounded chat answers include relevant indexed context and are measurably useful in dogfooding.
- Setup-to-first-response time for a configured environment is under 30 seconds.
- Existing CLI/MCP integration tests continue to pass with no behavior regressions.
- New `ctx chat` contract is documented in spec, CLI reference, and runbook.

## Dependencies and Risks

**Dependencies**
- [SPEC-0013](../spec/0013-direct-llm-inference-chat.md) for behavioral contract.
- Existing retrieval/index pipeline from current `search`/`get` architecture.
- Provider credentials and endpoint availability in environment/config.

**Risks**
- Provider API differences can increase abstraction complexity.
- Streaming UX differences across providers can cause inconsistent output.
- Prompt/context growth can affect latency and token cost.
- Users may expect tool-calling/agent-like behavior before it is implemented.

## Related Documents

- [SPEC-0013](../spec/0013-direct-llm-inference-chat.md) — Direct LLM Inference Chat.
- [ADR-0022](../adr/0022-direct-inference-provider-abstraction.md) — Provider abstraction decision.
- [DESIGN-0007](../design/0007-direct-inference-chat-implementation-plan.md) — Implementation plan.
- [RUNBOOK-0018](../runbook/0018-chat-via-cli.md) — Operational usage.
- [SPEC-0011](../spec/0011-mcp-agents.md) — Existing MCP agent/prompt model.
