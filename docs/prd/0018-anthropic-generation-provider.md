# PRD-0018: Claude (Anthropic) Generation Provider

**Status:** Draft
**Date:** 2026-11-09
**Author:** Context Harness contributors

## Problem Statement

Standalone agents already run against a provider-neutral model contract. The
built-in generation adapters are OpenAI Responses, native Ollama, and the `fake`
provider used by tests. An operator who wants Claude for those same agents has
no `[models.*]` provider to select. Pointing the OpenAI adapter at Anthropic is
not a Claude configuration: that adapter speaks the OpenAI Responses API, and
Anthropic's OpenAI-compatible surface is a different, narrower contract.

Claude Desktop and other Claude clients can already call Context Harness as an
MCP server. That path leaves inference in the client. It does not let
`ctx agent run`, a resumed run, or an embedded host use Claude as the model
behind an agent resource.

Operators need a first-party Anthropic generation adapter with the same bounded
turns, tool calls, history, cancellation, and classified errors as the adapters
that exist today. Credentials stay in the environment. Static validation must
not contact Anthropic or read the key.

## Target Users

- Agent authors who want a Claude model on an existing standalone agent without
  rewriting tools or prompts.
- Operators who already keep an Anthropic API key for other tools and want that
  key referenced from `[models.*]`, not copied into TOML.
- Embedding hosts that construct the built-in provider catalog and need Claude
  on the same factory path as OpenAI and Ollama.
- Contributors adding the adapter, who need a reviewable product scope before
  implementation.

## Goals

1. Select a Claude model with a `[models.*]` alias and run an existing agent
   through the Anthropic Messages API.
2. Keep the provider-neutral request, tool loop, history, cancellation, and
   error categories defined for the other built-in adapters.
3. Validate and inspect an Anthropic alias without a network call and without
   resolving the API key.
4. Fail with a distinct, non-secret error when the key is missing, the call is
   rejected, the model is unavailable, or the response cannot be used.
5. Preserve multi-turn tool calls on current Claude models, including provider
   reasoning blocks that the Messages API requires the client to send back.

## Non-Goals

- Streaming. [SPEC-0016](../spec/0016-model-runtime.md) defers streaming for
  every built-in adapter. This delivery stays non-streaming.
- Using Claude as an MCP client. That workflow already exists: Context Harness
  serves prompts and tools, and the MCP client owns inference. See
  [SPEC-0011](../spec/0011-mcp-agents.md).
- Choosing or pinning a default Claude model for the user.
- Prompt caching, effort, thinking display, server-side tools, images, files,
  the Batches API, or the Token Counting API.
- Amazon Bedrock, Google Vertex, Microsoft Foundry, or any transport other than
  the Anthropic Messages API.
- Changing embedding providers, retrieval, or the `[embedding]` table.
- Adding adapter-level retries beyond the bounded runtime policy already in
  SPEC-0016.
- A second provider id such as `claude`. Claude is the model family named in
  the `model` field.

## User Stories

- I add an Anthropic alias, export `ANTHROPIC_API_KEY`, and run my existing
  project researcher without changing its tools or system prompt.
- I run `ctx agent validate` and `ctx agent show` on a laptop that cannot reach
  `api.anthropic.com`. Validation succeeds and the output contains no API key.
- A tool turn that returns several calls still executes each tool and sends
  each result back before the next model turn.
- If the key is absent or Anthropic rejects the call, the run fails with a
  category I can tell apart from a malformed response, and the log does not
  contain the key or the raw error body.
- I keep using OpenAI and Ollama aliases in the same config file. Those aliases
  behave as they do today.

## Requirements

| ID | Requirement |
|---|---|
| A1 | A `[models.*]` alias can name the Anthropic generation provider, a Claude model id, and the environment variable that holds the API key. |
| A2 | The default credential variable is `ANTHROPIC_API_KEY`. The config stores the variable name, never the key. |
| A3 | An existing agent can switch to that alias without changes to its prompt, tool bindings, or permissions. |
| A4 | Multi-turn tool use works, including more than one tool call in a single model turn. |
| A5 | Provider-owned reasoning blocks required for a later turn are retained with the conversation and are not treated as the user-visible answer or as run metadata. |
| A6 | A requested output bound or structured JSON result is sent to Anthropic. If the model or the API cannot honor it, the call fails visibly. |
| A7 | Static validation and inspection make no network call, do not resolve the credential, and do not print the key. |
| A8 | Missing credentials, authentication failure, an unavailable model, rate limiting, overload, timeout, and a malformed response stay distinguishable. Diagnostics exclude the key and the raw error body. |
| A9 | Existing `openai`, `ollama`, and `fake` aliases keep their current behavior. |

## Success Criteria

- Fixture tests, with no live Anthropic call and no API key, cover a text
  reply, a multi-tool turn, credential failure, the error categories in A8,
  and a response that includes reasoning blocks the next turn must echo.
- `ctx agent validate` and `ctx agent show` succeed for an Anthropic alias
  while the network is unavailable, and their output does not contain a key
  value.
- An agent resource written for an OpenAI alias runs against an Anthropic
  alias with the same prompt and tool list.
- The OpenAI, Ollama, and fake model-runtime suites stay green.
- No streaming transport is added.

## Dependencies and Risks

This work depends on the provider-neutral types and recording loop in
[SPEC-0016](../spec/0016-model-runtime.md) and on the agent loop in
[SPEC-0017](../spec/0017-local-agent-execution.md). The Messages API requires
`max_tokens` on every call, while the neutral request leaves the output limit
optional. Current Claude models can attach reasoning blocks that a later tool
turn must echo unchanged. Non-streaming calls can time out on long turns; the
delivery still does not add streaming.

Public tool names such as `project.search` contain characters the Messages API
rejects in a tool name. The adapter has to keep those names working. Model
quality and account entitlements are operator concerns. Static validation does
not prove that a model id is enabled for the key.

## Related Documents

- [DESIGN-0019](../design/0019-anthropic-generation-provider.md): proposed
  adapter, wire mapping, and the open questions for review.
- [PRD-0015](0015-local-generation-providers.md): the provider-catalog product
  intent this adapter reuses.
- [SPEC-0016](../spec/0016-model-runtime.md): current model contract. The
  implementation change extends it; this PRD does not.
- [SPEC-0017](../spec/0017-local-agent-execution.md): bounded execution loop.
- [SPEC-0011](../spec/0011-mcp-agents.md): MCP personas, where Claude remains a
  client and Context Harness does not run inference.
- [ADR-0026](../adr/0026-native-ollama-local-generation.md): precedent for a
  native provider API instead of an OpenAI-compatible route.
