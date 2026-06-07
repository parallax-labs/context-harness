# ADR-0022: Direct Inference Provider Abstraction for `ctx chat`

**Status:** Proposed
**Date:** 2026-03-03

## Context

Context Harness currently exposes retrieval and agent prompt capabilities, while inference is performed by external MCP clients. The new `ctx chat` capability introduces direct LLM inference from the CLI, which requires provider integration for OpenAI-style APIs, Ollama, and compatible endpoints.

Without an abstraction boundary, provider-specific request/response and streaming behavior would leak into command logic, making it difficult to add providers and maintain consistent UX/error handling.

## Decision

We will introduce a dedicated provider abstraction for direct inference used by `ctx chat`:

1. A single chat-facing interface will normalize:
   - request construction (system/context/history/user messages),
   - streaming and non-streaming output,
   - error categories (`auth_error`, `provider_error`, `timeout_error`, etc.).
2. Provider implementations will be separate modules behind the interface:
   - OpenAI
   - Ollama
   - OpenAI-compatible endpoint
3. `ctx chat` command logic will depend only on the abstraction, not provider-specific HTTP shapes.
4. Retrieval grounding remains a separate pre-inference step and is not embedded into provider implementations.

## Alternatives Considered

### 1. Provider-specific code directly in `ctx chat` command

Rejected because it tightly couples CLI behavior to each provider, complicates testing, and makes future provider additions risky.

### 2. Adopt one provider only (OpenAI) in first release

Rejected because it conflicts with local-first and self-hosted usage patterns already present in Context Harness (e.g., Ollama support for embeddings).

### 3. Reuse MCP agent infrastructure for direct inference

Rejected because MCP agent flow is prompt/tool orchestration for external clients, not direct completion transport. This would blur concerns and violate existing stateless MCP model assumptions.

## Consequences

**Positive**
- Cleaner separation of command orchestration and provider transport.
- Easier to add providers with consistent UX and error handling.
- Better testability via interface-level mocks/fakes.
- Preserves existing MCP/agent architecture while adding CLI-native inference.

**Negative**
- Additional abstraction layer adds implementation overhead.
- Streaming normalization across providers can still require provider-specific edge handling.
- Early implementation may support only a subset of advanced provider features.

## References

- [PRD-0012](../prd/0012-direct-llm-inference-chat.md)
- [SPEC-0013](../spec/0013-direct-llm-inference-chat.md)
- [SPEC-0011](../spec/0011-mcp-agents.md)
