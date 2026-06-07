# SPEC-0013: Direct LLM Inference Chat

**Status:** Draft
**Date:** 2026-03-03
**Scope:** Direct chat inference from the `ctx` binary, including provider selection, retrieval grounding, streaming output, and error behavior.

## Overview

This spec defines a first-class chat workflow in the `ctx` CLI so users can chat directly from the binary without relying on an external MCP client. The chat workflow reuses existing retrieval and document APIs, then invokes a configured LLM provider to produce responses.

This spec adds a direct-inference path. It does not remove or replace MCP prompt/tool workflows defined in [SPEC-0011](0011-mcp-agents.md).

## Definitions

- **Direct inference:** The `ctx` process calls the configured LLM provider API and receives generated tokens directly.
- **Chat turn:** One user prompt plus one assistant response.
- **Session:** Ordered chat turns, including a system prompt and optional retrieval context.
- **Grounded mode:** Chat mode that retrieves context from the local index before inference.
- **Ungrounded mode:** Chat mode that performs no retrieval and sends only conversation history to the model.

## 1. CLI Surface

### R1: `ctx chat` command

The CLI SHALL provide a `chat` command:

```bash
ctx chat [PROMPT]
```

If `PROMPT` is present, the command SHALL execute one chat turn and exit. If omitted, the command SHALL start an interactive REPL session.

### R2: Required flags

`ctx chat` SHALL support the following flags:

| Flag | Type | Default | Behavior |
|------|------|---------|----------|
| `--mode` | enum | `grounded` | `grounded` or `ungrounded` |
| `--provider` | string | from config | Overrides configured LLM provider |
| `--model` | string | from config | Overrides configured model |
| `--system` | string | empty | Prepends a system instruction for the session |
| `--limit` | int | retrieval default | Max retrieved items in grounded mode |
| `--source` | string | all sources | Retrieval source filter in grounded mode |
| `--stream` / `--no-stream` | bool | `--stream` | Enable or disable token streaming |
| `--json` | bool | false | Emit machine-readable JSON output for single-turn mode |
| `--save-session <path>` | path | unset | Write session transcript as JSONL |
| `--load-session <path>` | path | unset | Resume a prior session transcript |

### R3: Exit behavior

- In single-turn mode, `ctx chat "<prompt>"` SHALL exit with status code 0 on success.
- In interactive mode, `Ctrl-D` SHALL end the session with status code 0.
- Provider/config/runtime failures SHALL exit non-zero.

## 2. Configuration

### R4: LLM config section

The config schema SHALL include an `[llm]` section:

```toml
[llm]
provider = "openai"      # openai | ollama | openai-compatible
model = "gpt-4o-mini"
base_url = "https://api.openai.com/v1"   # optional for openai, required for openai-compatible
api_key_env = "OPENAI_API_KEY"           # env var name containing API key
temperature = 0.2
max_tokens = 1024
timeout_secs = 120
```

### R5: Resolution and overrides

Provider and model resolution SHALL use this priority:

1. CLI flags (`--provider`, `--model`)
2. `[llm]` config values
3. Error if unresolved

### R6: Backward compatibility

Existing configs without `[llm]` SHALL remain valid for non-chat commands. Running `ctx chat` without a resolved provider/model SHALL return a clear configuration error with setup guidance.

## 3. Retrieval Grounding

### R7: Grounded turn composition

When `--mode grounded` is active, each turn SHALL:

1. Run retrieval using existing search behavior (default hybrid mode, configurable source filter, and limit).
2. Build a context block from retrieved items (title/source/snippet and document IDs).
3. Include this context block in the LLM request before the user prompt.

### R8: Retrieval transparency

In interactive mode, the CLI SHALL print a compact retrieval summary before the assistant response, including:

- Retrieval mode
- Number of context items included
- Source filter (if any)

When `--json` is enabled, retrieved context metadata SHALL be included in the response payload.

## 4. Inference Behavior

### R9: Message ordering

LLM requests SHALL use this message order:

1. Optional system instruction (`--system`)
2. Prior chat history in chronological order
3. Grounding context (if grounded mode)
4. Current user prompt

### R10: Streaming

By default, assistant output SHALL stream token-by-token to stdout. With `--no-stream`, output SHALL be buffered and printed once complete.

### R11: Deterministic transcript format

If `--save-session` is provided, each turn SHALL append JSONL records with at least:

- `role` (`system`, `user`, `assistant`)
- `content`
- `timestamp`
- `provider`
- `model`

If grounded mode is used, assistant records SHALL also include the retrieved document IDs for that turn.

## 5. Error Handling

### R12: Error categories

`ctx chat` SHALL classify and report errors in these categories:

- `config_error` (missing/invalid provider config)
- `auth_error` (missing or rejected credentials)
- `provider_error` (non-2xx response from provider)
- `timeout_error` (request timeout reached)
- `rate_limit_error` (provider throttling)
- `context_error` (retrieval failure in grounded mode)

### R13: Human-readable and JSON output

- Default output SHALL provide concise human-readable errors.
- With `--json`, errors SHALL be emitted as JSON objects with `code`, `message`, and optional `details`.

## 6. Relationship to Existing Interfaces

### R14: MCP and agent compatibility

The direct-inference CLI path SHALL coexist with MCP tools and agent prompts. `ctx serve mcp` behavior SHALL remain unchanged.

### R15: Contract update

This spec introduces a new public CLI contract entry (`ctx chat`) and SHALL be reflected in:

- [SPEC-0005](0005-usage-contract.md)
- CLI reference docs (`site/content/docs/reference/cli.md`)

before status is changed to Authoritative.

## Acceptance Criteria

1. `ctx chat "hello"` with a valid `[llm]` config returns an assistant response and exits 0.
2. `ctx chat` starts an interactive session and supports multiple turns until EOF.
3. `ctx chat --mode grounded` includes retrieval context and shows retrieval summary.
4. `ctx chat --mode ungrounded` performs no retrieval.
5. `ctx chat --json "hello"` returns valid JSON with response text and metadata.
6. `ctx chat --save-session session.jsonl` writes session records that can be resumed with `--load-session`.
7. Missing provider credentials produce `auth_error` with actionable guidance.
8. Existing commands (`sync`, `search`, `serve mcp`, `agent test`, etc.) behave unchanged when `[llm]` is absent.
