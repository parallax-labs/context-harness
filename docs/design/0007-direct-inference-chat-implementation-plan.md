# DESIGN-0007: Direct Inference Chat Implementation Plan

**Status:** Planning
**Date:** 2026-03-03
**Author:** Parker Jones
**Related:** [PRD-0012](../prd/0012-direct-llm-inference-chat.md), [ADR-0022](../adr/0022-direct-inference-provider-abstraction.md), [SPEC-0013](../spec/0013-direct-llm-inference-chat.md), [SPEC-0011](../spec/0011-mcp-agents.md)

## Context

Context Harness needs a native terminal chat workflow (`ctx chat`) that performs direct inference and optionally grounds responses in indexed context. The system already has robust retrieval, document APIs, and MCP prompt/tool flows, but no in-process completion path.

This design defines implementation boundaries, phased delivery, and verification strategy for `ctx chat`.

## Proposal

### 1. Architecture

Implement chat as a CLI command with three layers:

1. **Session Orchestrator (CLI layer)**
   - Parses flags and resolves config/overrides.
   - Manages interactive loop and transcript persistence.
   - Invokes retrieval in grounded mode.
2. **Prompt Composer**
   - Builds normalized message array from system prompt, history, context block, and user input.
   - Applies token budget safeguards for context/history truncation.
3. **Provider Adapter Layer**
   - Executes inference (streaming and non-streaming).
   - Maps provider errors into normalized error categories.

### 2. Retrieval grounding integration

- Grounded mode calls existing search pipeline with a configurable limit/source.
- Retrieved items are transformed into a compact context block containing title/source/snippet and document ID.
- Context block is injected as a dedicated message segment before user input.

### 3. Output and transcript behavior

- Streaming is default to improve perceived latency.
- `--json` is supported for single-turn scripting.
- Session transcript format is JSONL for append-only durability and easy post-processing.

### 4. Backward compatibility

- `ctx serve mcp` and agent prompt APIs remain unchanged.
- No changes to existing tool contracts required for initial `ctx chat` release.

## Alternatives Considered

### 1. Add a chat completion HTTP endpoint first

Not chosen for first delivery because the user request is binary-native chat and a server endpoint adds security/surface-area concerns before core CLI UX is validated.

### 2. Build tool-calling agent loop into `ctx chat` immediately

Not chosen for first delivery to keep initial scope focused on reliable direct inference and retrieval grounding.

### 3. Implement non-streaming only

Not chosen because streaming is materially better UX for terminal chat and required by the spec.

## Implementation Plan

1. **Config and models**
   - Add `[llm]` config schema and validation.
   - Add runtime override resolution for provider/model flags.

2. **Provider abstraction**
   - Define provider trait/interface and normalized error type.
   - Implement adapters for OpenAI, Ollama, and OpenAI-compatible endpoints.

3. **Chat command**
   - Add `ctx chat [PROMPT]` command and flags.
   - Implement single-turn and interactive loop.
   - Add streaming and non-stream output paths.

4. **Grounding**
   - Integrate retrieval call in grounded mode.
   - Add retrieval summary output and JSON metadata.

5. **Session persistence**
   - Implement `--save-session` and `--load-session` JSONL support.

6. **Tests**
   - Unit: config resolution, prompt composition, transcript encoding.
   - Integration: provider mocks for streaming/non-streaming and error mapping.
   - Regression: verify existing CLI and MCP commands unchanged.

7. **Docs**
   - Update usage contract and CLI reference for `ctx chat`.
   - Publish operator runbook for terminal chat workflows.

## Open Questions

1. Should token budgeting be fixed-size defaults only, or expose explicit CLI flags for context/history budgets in v1?
2. Should interactive mode support slash commands (`/mode`, `/save`, `/clear`) in initial release or follow-up?
3. Should transcript files include retrieval snippets or only document IDs for privacy and file-size control?
