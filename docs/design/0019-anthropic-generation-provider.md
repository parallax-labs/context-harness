# DESIGN-0019: Claude (Anthropic) Generation Provider

**Status:** Draft
**Date:** 2026-11-09
**Author:** Context Harness contributors
**Related:** [PRD-0018](../prd/0018-anthropic-generation-provider.md), [PRD-0015](../prd/0015-local-generation-providers.md), [DESIGN-0015](0015-local-generation-providers.md), [ADR-0026](../adr/0026-native-ollama-local-generation.md), [SPEC-0016](../spec/0016-model-runtime.md), [SPEC-0017](../spec/0017-local-agent-execution.md), [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md), [SPEC-0011](../spec/0011-mcp-agents.md)

This document is a planning artifact. It is not the behavioral contract.
[SPEC-0016](../spec/0016-model-runtime.md) stays authoritative until the
implementation change extends it. Official Messages API behavior below was
checked on 2026-11-09 against the Claude API docs on `platform.claude.com`,
the current site for the docs published under `docs.claude.com` and
`docs.anthropic.com`. Citations are in [References](#references).

## Context

`ModelProvider::generate` is provider-neutral. `ModelProviderCatalog` builds
`openai`, `fake`, and `ollama` from `[models.*]`. `ModelDefinition` already
carries `provider`, `model`, `api_key_env`, `base_url`, and `timeout_seconds`.
OpenAI and fake reject `base_url` and `timeout_seconds`. Ollama rejects
`api_key_env` and accepts only a literal loopback `base_url`.

The agent loop appends `ModelResponse::message()` to the next request, and that
message already carries `ProviderContinuation`. OpenAI uses that continuation
to replay reasoning items it must not rebuild. The Anthropic adapter can use
the same path. No second agent loop is required.

[SPEC-0016](../spec/0016-model-runtime.md) fixes the neutral finish reasons,
the three-attempt runtime retry policy, the 4 MiB response cap used by the
existing HTTP adapters, and the rule that events carry error category and
optional HTTP status only. Streaming is deferred. This design stays inside
that contract.

Claude-as-MCP-client is out of scope. [SPEC-0011](../spec/0011-mcp-agents.md)
already leaves inference with the MCP client.

## Proposal

### Provider identity

The configuration provider id is `anthropic`.

```toml
[models.claude]
provider = "anthropic"
model = "claude-sonnet-5-5" # illustrative; the operator chooses a model id
api_key_env = "ANTHROPIC_API_KEY"
```

`anthropic` names the API, the same way `openai` and `ollama` name APIs. The
model id remains whatever the account can call (`claude-sonnet-5-5`,
`claude-opus-5-5`, and the rest of the current model list). The provider id
`claude` is not registered and is not accepted as an alias. Provider names stay
case-sensitive, so `Anthropic` is an unknown provider.

The factory implementation id is `context-harness.anthropic-messages`, version
`1`. The version bumps if the wire contract becomes checkpoint-incompatible,
including a change of the pinned `anthropic-version`.

### Catalog fit

Add `AnthropicFactory` to `ModelProviderCatalog::with_builtins` beside the
three existing factories. `ModelRegistry::from_config` keeps going through the
catalog. There is no new `match` on provider names in the agent loop.

The factory validates a definition without reading the environment, opening the
database, or opening a socket. `build` constructs the provider and still does
not read the key. The key is read on `generate`, and an empty or missing value
returns `missing_credentials` before any HTTP request. That matches
`OpenAiProvider`.

Suggested module layout, mirroring OpenAI:

- `crates/context-harness/src/agent_model/anthropic.rs`
- `crates/context-harness/src/agent_model/anthropic/tests.rs`

The module is a `reqwest` client. This design does not add an Anthropic SDK
crate. OpenAI and Ollama already speak HTTP through `reqwest`, and a generated
SDK would move the wire contract into a dependency release.

### Configuration and validation

Accepted keys are the existing `ModelDefinition` fields. `deny_unknown_fields`
already rejects anything else. No new TOML field is proposed for v1.

| Field | Anthropic behavior |
|---|---|
| `provider` | Required. Must be `anthropic`. |
| `model` | Required non-empty string. Passed through as the Messages `model`. The adapter does not check the id against a model list. |
| `api_key_env` | Optional environment-variable name. Default `ANTHROPIC_API_KEY`. The existing name grammar on `ModelDefinition` applies. The value is resolved only inside `generate`. |
| `base_url` | Optional HTTPS origin. Default `https://api.anthropic.com`. |
| `timeout_seconds` | Optional total timeout. Default 60. Allowed range `1..=3600`. Connect timeout stays 10 seconds. |

`base_url` is the one cloud-provider setting OpenAI does not have. It exists so
an operator can point at an HTTPS gateway that serves `POST /v1/messages`. It
is not a Bedrock, Vertex, or Foundry adapter. Those APIs use different request
shapes and signing. Validation:

- scheme `https`;
- host present, no userinfo, no query, no fragment;
- path empty or `/`;
- a nonzero port when one is explicit.

The adapter then sets the path to `/v1/messages`. Fixture tests inject the
endpoint on the provider struct, as the OpenAI tests do, so config validation
can stay HTTPS-only without a loopback exception. Redirects are disabled.

OpenAI and fake keep rejecting `base_url` and `timeout_seconds`. The error text
in `OpenAiFactory` and `FakeFactory` currently says those keys are only
supported by Ollama. The implementation updates that sentence so it names the
providers that accept the keys. Ollama keeps rejecting `api_key_env`.

`timeout_seconds` is configurable because a non-streaming Messages call can run
long enough for Anthropic to return HTTP 504, and the errors guide tells
clients to use streaming for those calls. Streaming stays deferred, so the
operator gets a timeout knob instead. Sixty seconds matches the OpenAI
adapter's fixed timeout and remains the default. The upper bound matches
Ollama's `1..=3600` check.

Checkpoint binding already hashes the raw `ModelDefinition`, including the
credential variable name and excluding the secret value
([SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md)). The
implementation does not rewrite that digest and does not persist defaulted
`base_url` or `timeout_seconds` back into the stored definition. Filling
defaults into the hashed table would make an omitted default and an explicit
default look like different bindings. Request-time defaults are enough.

### Transport

One `POST` per `generate`. The URL is the validated origin plus `/v1/messages`.
The client uses `redirect: none`, a 10 second connect timeout, and the
configured total timeout. Unlike Ollama, the client does not set `no_proxy`:
the process proxy environment should apply, as it does for OpenAI.

Headers on every request:

| Header | Value |
|---|---|
| `authorization` | `Bearer <key>` |
| `anthropic-version` | `2026-10-21` |
| `content-type` | `application/json` |

The API overview marks `anthropic-version` required and shows `2026-10-21` as
the current version. The authentication guide prefers `Authorization: Bearer`
and still accepts the legacy `x-api-key` header. This adapter sends Bearer
only. It does not send `x-api-key`, `anthropic-beta`, or
`anthropic-workspace-id`.

The version is a constant, not a config key. API version `2026-10-21` rejects
`temperature`, `top_p`, and `top_k`, so the adapter sends none of them.
Omitting them samples with Anthropic's defaults. The adapter also sends
`stream: false` and does not send `metadata`, `container`, `inference_geo`,
`service_tier`, `speed`, `stop_sequences`, or `tool_choice`.

The overview says a key that is not scoped to one workspace must send
`anthropic-workspace-id`. v1 leaves that header off. A workspace-scoped key
works without it. An unscoped key that Anthropic rejects comes back as HTTP
403 and is classified below. Whether v1 should grow a non-secret workspace id
is an [open question](#open-questions).

### Request mapping

`generate` calls `ModelRequest::validate` first. The neutral rules still own
duplicate call ids, unmatched tool results, and outstanding results. The
adapter adds the Messages-specific checks below and fails with
`invalid_request` before it builds a body.

`max_tokens` is a required Messages field (the create-message reference lists
it without an optional marker; minimum 0). Zero means "write the prompt cache
and do not generate." The neutral `max_output_tokens` of 0 is already rejected.
Mapping:

- `Some(n)` sends `max_tokens: n`.
- `None` sends `max_tokens: 4096`.

4096 is a recommendation, not a locked product requirement. Thinking tokens
count toward `max_tokens`, so the default has to leave room for both reasoning
and the visible answer. A model that rejects the value returns HTTP 400, which
stays `invalid_request`. The adapter does not clamp. See
[open questions](#open-questions).

System messages are not a Messages role on the common path. The runtime's
compatibility projection starts with the agent system prompt and then the user
input. Bounded projection inserts a second system capsule before that user
message. The adapter concatenates a leading prefix of `System` messages, in
order, skipping empty content, and joins the remaining segments with a blank
line. That string is the top-level `system` field. If nothing remains, the
field is omitted. A `System` message that appears after a user or assistant
message fails locally as `invalid_request`. Mid-conversation `role: "system"`
is documented only for a subset of current models and cannot be the first
message, and the runtime does not emit it today.

Other messages:

| Neutral message | Messages body |
|---|---|
| `User { content }` | `{ "role": "user", "content": "<text>" }` |
| `Assistant` without continuation | One assistant message. Content is a `text` block when `text` is non-empty, then one `tool_use` block per call. |
| `Assistant` with an `anthropic` continuation | That stored `content` array, after the integrity check below. |
| One or more `Tool` messages that settle the preceding assistant turn | One user message whose `content` is the `tool_result` blocks in the same order. |

Text blocks have a minimum length of 1, so an empty assistant string does not
become a text block. `tool_use` is `{ "type": "tool_use", "id", "name", "input" }`.
`id` is the neutral call id. `name` is the wire name from the table below.
`input` is the neutral arguments object.

`tool_result` is `{ "type": "tool_result", "tool_use_id", "content" }` with the
neutral result string. The adapter does not set `is_error`; the neutral tool
message has no error flag. Anthropic requires every `tool_use` to be answered
by a `tool_result` in the immediately following user message, with those blocks
before any text. Grouping consecutive tool results into one user message
satisfies that. The neutral validator already requires one result per call
before the next turn, so the group is unambiguous. Several `tool_use` blocks in
one assistant message are the parallel-call case, and each gets its own
`tool_result`.

Call ids round-trip. The adapter does not mint ids. Anthropic's `tool_use.id`
must match `^[a-zA-Z0-9_-]+$`. An id outside that pattern fails as
`invalid_request` before the request is sent. Ids Anthropic generates
(`toolu_...`), Ollama's `ollama-call-<ordinal>-<index>`, and typical OpenAI
`call_...` ids match. An id that contains `.` or `:` does not. The adapter
does not rewrite ids: a rewritten id would no longer match the `tool_use` that
produced it.

Tool names on the wire use the existing SHA-256 helper, `wire_name`. The digest
is 64 hex characters, which matches Anthropic's tool-name pattern
`^[a-zA-Z0-9_-]{1,128}$`. Public names such as `project.search` do not match,
because of the dot. OpenAI hashes for the same reason. The description sent to
Anthropic is `"{public name}: {description}"`, so the model still sees the
name the agent uses. Response names map back only through the hash table of
tools declared on this request. The Ollama adapter also accepts an unhashed
echo of the public name. This adapter does not. Anthropic returns the name it
was given.

Each tool definition is:

```json
{
  "name": "<wire_name>",
  "description": "<public name>: <description>",
  "input_schema": { }
}
```

`input_schema` is the neutral parameters object, unchanged. `strict` is
omitted. Strict tool use compiles a grammar and rejects JSON Schema features
the agent schemas still use. OpenAI sends `strict: false` for the same reason.
An empty tool list omits `tools`. The adapter does not send server tools,
toolsets, or `defer_loading`.

A named `OutputSchema` becomes:

```json
{
  "output_config": {
    "format": { "type": "json_schema", "schema": { } }
  }
}
```

The neutral schema object is `schema`. The neutral schema name stays on the
request and is not a Messages field. When the request has both tools and an
output schema, the adapter sends both. Anthropic documents that JSON outputs
and tool use can be combined. The adapter does not drop either. It does not
rewrite the schema to add `additionalProperties: false`. A schema the API
rejects is a failed call, not a silent retry without the schema.

Continuation integrity matches the OpenAI rule. A continuation whose provider
is not `anthropic` fails as `invalid_request`. A continuation that decodes to
different assistant text or tool calls than the neutral message fails the same
way, before HTTP. On success, the stored content array is the assistant
`content` sent back. Rebuilding that array from text and tool calls would drop
`thinking` and `redacted_thinking` blocks.

Those blocks are why continuation is required, not optional polish. On the
Messages API, thinking is on by default for Claude 5 and later models. The
tool-use guide says the next request must echo the assistant `content` array
unchanged, including `thinking` and `redacted_thinking`. Filtering those blocks
is a 400. `thinking: {"type": "disabled"}` is not a portable substitute:
Claude Sonnet 5.5 rejects it. This adapter therefore omits `thinking` and
preserves whatever blocks come back. It does not set `display`. On models whose
display defaults to `omitted`, the thinking text may be empty and the
`signature` is still required on replay.

Serialized request bodies larger than 4 MiB fail as `invalid_request` and are
not sent. Anthropic's own Messages limit is 128 MB. The 4 MiB cap matches
Ollama and keeps one adapter call inside the memory bound the runtime already
uses.

### Response mapping

A success body must be a message: `type` is `message`, `role` is `assistant`,
and `content` is an array. The adapter walks that array in order.

| Block | Result |
|---|---|
| `text` | Appended to the neutral response text. |
| `tool_use` | One `ToolCall`. `id` and `input` are taken from the block. `name` must be a wire name declared on this request. `input` must be a JSON object. |
| `thinking`, `redacted_thinking` | Kept in the continuation array. Not copied into `text`, events, or `Debug`. |
| Any other block | `invalid_response`. The adapter does not drop `server_tool_use` or other server blocks. |

`stop_reason` in a non-streaming response is non-null. Mapping:

| `stop_reason` | Neutral finish | Calls |
|---|---|---|
| `end_turn` | `completed` when there is no `tool_use` and the text is non-empty. Empty text is `invalid_response`. | none |
| `tool_use` | `tool_calls` when at least one `tool_use` block was accepted. Zero calls is `invalid_response`. | the accepted calls |
| `max_tokens` | `length` | cleared, including any partial `tool_use` |
| `model_context_window_exceeded` | `length` | cleared |
| `refusal` | `refusal` | cleared |
| `stop_sequence` | `invalid_response`. The adapter does not send stop sequences. | none |
| `pause_turn` | `invalid_response`. This is a server-tool pause. The adapter does not continue it. | none |
| anything else | `invalid_response`. The versioning policy allows new enum values. | none |

`completed` and `tool_calls` store the raw `content` array in
`ProviderContinuation` with provider `anthropic`. `length` and `refusal` store
no continuation, matching the OpenAI rule for incomplete and refused turns.
`stop_details` is not copied into the response text, the error display, or run
events. The refusal category and explanation are unstable policy diagnostics.

There is no Messages `content_filter` stop reason. The adapter does not invent
`content_filter` from the text.

Usage, when the object is present:

- input tokens are `input_tokens` plus `cache_creation_input_tokens` plus
  `cache_read_input_tokens`, treating a null cache counter as zero;
- output tokens are `output_tokens`;
- total is the checked sum of those two.

Anthropic's usage object says the billable input total is the sum of the
uncached input and the two cache counters. Folding them in keeps a budget from
undercounting a cache hit. `output_tokens` already includes thinking tokens.
The neutral `Usage` struct does not grow cache fields. A missing `usage`
object stays `usage: None`, not zeros. Checked addition overflow is
`invalid_response`.

For `completed` with an output schema, the adapter parses `text` as JSON and
sets `structured_output` to that value. The existing neutral check still
requires the parsed value to equal that JSON. Other finish reasons leave
`structured_output` empty. A `tool_use` stop is not required to be schema JSON.

### Errors, limits, and retries

The adapter performs no retries. [SPEC-0016](../spec/0016-model-runtime.md)
already retries `invalid_response`, `rate_limited`, `unavailable`, `timeout`,
and `transport` at most three times per turn, with 250 ms then 750 ms delays.
Classification chooses a kind the runtime already knows.

Error bodies are not read, logged, stored, or placed on `ModelError`. They can
echo prompt text. OpenAI already classifies by status alone. This adapter does
the same. `ModelError` remains `{ kind, http_status }`. The `request-id`
header is not added to events in v1.

| HTTP status | `ModelErrorKind` | Runtime retries |
|---|---|---|
| 401, 403 | `authentication` | no |
| 402 | `provider_failure` | no |
| 404 | `model_unavailable` | no |
| 409 | `provider_failure` | no |
| 413 | `invalid_request` | no |
| 429 | `rate_limited` | yes |
| 500–503, 505–599, 529 | `unavailable` | yes |
| 504 | `timeout` | yes |
| 400, 422 | `invalid_request` | no |
| any other non-success | `provider_failure` | no |
| redirect status, if one is observed | `provider_failure` | no |

A spend-cap HTTP 429 has no useful `retry-after` and will fail again. The
existing policy still spends the two extra attempts. That is accepted for v1
rather than adding a new error kind. HTTP 529 `overloaded_error` maps to
`unavailable` so the same policy retries it. Connection failure and other
`reqwest` errors map like OpenAI: a timeout is `timeout`, and anything else is
`transport`. A response body above 4 MiB is `invalid_response` and the rest of
the body is discarded.

The adapter does not sniff `error.message` for `unsupported_capability`. That
message can quote request content. A model that rejects tools or
`output_config` returns 400 and is `invalid_request`, which the runtime does
not retry.

Cancellation stays the runtime's job. Dropping the `generate` future drops the
in-flight HTTP call and is not recorded as a provider failure. The adapter
does not add its own timeout task.

### Static inspection

`ctx agent validate` and catalog validation stay offline. They do not resolve
`api_key_env`.

`ctx agent show` today prints base URL, timeout, and `local_only` only for
Ollama. If the configurable `base_url` in this design is accepted, show gains
the canonical Anthropic origin and the effective timeout for an `anthropic`
alias, with `local_only` absent. It still does not print the key or the
environment-variable value. If the maintainer instead hardcodes the endpoint,
show stays as it is for OpenAI and prints no extra endpoint lines.

### Test plan

Tests live next to the adapter and follow `agent_model/openai/tests.rs`: an
in-process axum fixture, a captured request, and no live Anthropic call. The
suite asserts these cases.

- Text reply: `model`, `stream: false`, `anthropic-version: 2026-10-21`,
  `Authorization: Bearer`, top-level `system` from a leading system prefix,
  user content, and no `temperature`, `top_p`, or `top_k`.
- Default `max_tokens` of 4096 when `max_output_tokens` is absent, and the
  request value when it is set.
- Dotted tool name hashed to 64 hex characters, description prefixed with the
  public name, and the response wire name mapped back. An unknown wire name
  fails.
- Two `tool_use` blocks in one assistant message become two neutral calls. The
  next body groups both `tool_result` blocks into the following user message,
  in order, with the original ids.
- A historical call id outside `^[a-zA-Z0-9_-]+$` fails before HTTP.
- A `thinking` block plus `tool_use` is stored as continuation, echoed
  verbatim on the next turn, and omitted from `Debug` and from the neutral
  text. A `redacted_thinking` block is kept. Tampering the neutral text or
  attaching a foreign continuation fails before HTTP.
- `end_turn`, `tool_use`, `max_tokens` (calls cleared), `refusal` (calls
  cleared, `stop_details` absent from `Display`), `model_context_window_exceeded`,
  and an unknown stop reason.
- Structured output sets `output_config.format` and parses a completed text
  body. Invalid JSON fails. Tools and the schema are both present when both
  were requested.
- Usage folds both cache counters into input tokens. Missing usage stays
  absent. Overflow fails.
- Status mapping for 401, 403, 404, 413, 429, 500, 504, and 529. The error
  `Display` and `Debug` do not contain a fixture secret planted in the body.
  The fixture sees one request.
- Missing credentials send no request.
- Timeout, oversized response, oversized request, and a redirect that is not
  followed.
- Config validation accepts the defaults, rejects HTTP, userinfo, a non-root
  path, a query, and a timeout outside `1..=3600`, and does not read the
  environment. Catalog identity is `context-harness.anthropic-messages`
  version `1`.
- Existing OpenAI, Ollama, and fake tests stay green, including whatever
  assertion covers the updated "which providers accept `base_url`" error.

`crates/context-harness/tests/agent_models.rs` gains an offline catalog case
next to the Ollama identity case. No paid or network test is part of the
required suite. SPEC-0016 already says the OpenAI contract does not require a
live provider call.

### Documentation in the implementation change

This design PR does not edit the spec, an ADR, the site, or the skill. Those
land with the code, once the open questions are resolved:

- Extend [SPEC-0016](../spec/0016-model-runtime.md) with an Anthropic section
  written in the same normative style as the OpenAI and Ollama sections:
  config shape, headers, message mapping, stop reasons, usage, body caps, and
  the fixture list. Update the scope sentence that currently names fake,
  OpenAI Responses, and Ollama. Bump the "checked against" date.
- Add the next ADR (0034 as of 2026-11-09, or whatever number is free at
  implementation time) recording the native Messages API, the `anthropic`
  provider id, and the `2026-10-21` pin. [ADR-0026](../adr/0026-native-ollama-local-generation.md)
  is the precedent. The ADR is written when the decision is accepted, not in
  this draft.
- `site/content/docs/reference/configuration.md`: an Anthropic example beside
  the Ollama example. The `[models.*]` row already exists.
- `site/content/docs/agents/build-local-agents.md`: the sentence that lists
  the built-in generation adapters.
- `skills/context-harness-agents/references/resources.md`: the sentence that
  says the built-in runtime supplies an OpenAI adapter. The bootstrap script
  and `examples/local-agents/config.toml` stay on OpenAI. The skill learns
  that Anthropic is available; it does not become the default.

## Alternatives Considered

**Reuse the OpenAI adapter against Anthropic's OpenAI-compatible base URL.**
Anthropic documents that layer as a way to compare models, and says it is not
the long-term production API. It speaks Chat Completions. The OpenAI adapter
speaks `POST /v1/responses` and will not parse Messages payloads. The
compatibility layer also ignores function-calling `strict` and hoists system
messages itself. A native adapter keeps tool ids, stop reasons, and thinking
blocks visible.

**Register `claude` as the provider id, or accept both ids.** `claude` is the
model family, and the model string already starts with it. Two ids for one
factory would make checkpoint identity and docs diverge. `anthropic` matches
the credential name `ANTHROPIC_API_KEY` and the API host.

**Pin `anthropic-version: 2023-06-01`.** That version still exists. The current
overview tells clients to send `2026-10-21`, and that version removes sampling
parameters this adapter does not need. Staying on the 2023 version would freeze
the client on the legacy contract.

**Make `anthropic-version` configurable.** A version switch changes which
fields are rejected and which stop reasons exist. The pin lives in the adapter.
Changing it bumps the implementation version.

**Send `thinking: {"type": "disabled"}` to avoid continuation.** Current model
docs reject that value on Claude Sonnet 5.5, and the tool-use guide requires
echoing thinking blocks when they are present. Preserving the content array is
the portable behavior. Thinking display and effort stay out of v1.

**Hash tool names only when the public name is illegal.** Legal names would
appear unchanged and dotted names would appear as hashes. The same tool would
have two wire identities across providers. Always hashing matches OpenAI and
Ollama.

**Classify `unsupported_capability` by reading `error.message`.** Ollama does
this for a small set of server strings. Anthropic error messages can quote the
request. Status-only classification avoids that read. Capability rejection
stays `invalid_request`.

**Add the official Anthropic SDK.** The SDK chooses retries, the version
header, and error types on its own release cycle. A `reqwest` adapter keeps
the same control the OpenAI and Ollama adapters already have.

**Implement streaming in this slice.** SPEC-0016 defers it. The 504 risk is
handled with `timeout_seconds` and the `timeout` error kind.

## Implementation Plan

1. Add `AnthropicFactory`, validation, and catalog registration. Update the
   OpenAI and fake error text that still says `base_url` and `timeout_seconds`
   belong only to Ollama. Land the offline validation and identity tests.
2. Implement request and response mapping, continuation replay, and the
   fixture suite listed above. Keep the HTTP client to one request, with the
   header, body-cap, timeout, and redirect behavior in this design.
3. Extend static show/inspection only if configurable `base_url` is accepted.
4. In the same change as the code, extend SPEC-0016, add the ADR, and update
   the site page, the build-local-agents sentence, and the skill reference.
   Do not add a live paid test.

No agent-loop change is expected. The loop already retains continuation on
`ModelResponse::message()`.

## Acceptance Criteria

- A fixture-backed provider completes a text turn and a two-call tool turn,
  including a thinking block that is echoed and absent from diagnostics.
- Missing credentials, 401, 404, 429, 504, 529, timeout, oversized bodies, and
  redirects classify as this design says, with one HTTP attempt inside the
  adapter.
- Static validation does not read the environment or open a socket.
- OpenAI, Ollama, and fake suites pass without behavior changes other than the
  corrected validation error text.
- SPEC-0016 describes the shipped behavior, and the site and skill mention the
  provider without making it the default.

## Risks

- A future Claude model can emit a content block this design treats as
  `invalid_response`. Failing closed is deliberate. The implementation version
  and the spec change together if a new block becomes required.
- Thinking signatures live in continuation, which checkpoints already persist
  for OpenAI reasoning items. Events and `Debug` must keep omitting those
  items. Signatures are not credentials, and they are still not log material.
- The 4 MiB request cap can reject a transcript the 16 MiB checkpoint limit
  would still store. That is the same bound Ollama uses.
- Non-streaming 504s remain possible on long turns. `timeout_seconds` and the
  runtime retry are the mitigation until streaming exists.
- The 4096 default can be tight once thinking tokens count toward `max_tokens`.
  That default is an open question below.
- An unscoped API key can receive HTTP 403 until the operator uses a
  workspace-scoped key or a later change sends `anthropic-workspace-id`.

## Open Questions

1. Confirm the provider id `anthropic`, with no `claude` alias.
2. Is the omitted-output-limit default of `max_tokens: 4096` acceptable?
   Thinking tokens count toward it. The alternatives are to require
   `max_output_tokens` on Anthropic requests, or to add a provider config
   field. This design recommends 4096 and no new field.
3. Should `base_url` be an optional HTTPS origin, defaulting to
   `https://api.anthropic.com`? The alternative is to hardcode the host the
   way OpenAI does and keep a test-only endpoint seam. This design recommends
   the optional origin.
4. Confirm the status map: 404 to `model_unavailable`, 504 to `timeout`, 529
   and other 5xx to `unavailable`, 402 to `provider_failure`, and no
   `error.message` sniffing.
5. Confirm `model_context_window_exceeded` maps to `length` with tool calls
   cleared.
6. Confirm cache-creation and cache-read tokens fold into
   `usage.input_tokens`, leaving the neutral `Usage` struct unchanged.
7. Should v1 omit `anthropic-workspace-id` and require a workspace-scoped key?
   This design recommends omission. The alternative is an optional non-secret
   workspace id on the model definition, which every other provider would
   reject.
8. Confirm the implementation change records the accepted decisions in a new
   ADR as well as in SPEC-0016. This design recommends the ADR, following
   ADR-0026.

## References

Checked 2026-11-09:

- [Create a Message](https://platform.claude.com/docs/en/api/messages/create):
  `POST /v1/messages`, required `max_tokens` (minimum 0, where 0 does not
  generate), top-level `system`, `tool_use.id` pattern `^[a-zA-Z0-9_-]+$`,
  tool name pattern `^[a-zA-Z0-9_-]{1,128}$`, stop reasons, and usage.
- [API overview](https://platform.claude.com/docs/en/api/overview): required
  `anthropic-version` and `content-type`; `Authorization: Bearer` unless
  `x-api-key` is set; workspace header rules.
- [Authentication](https://platform.claude.com/docs/en/manage-claude/authentication):
  Bearer as the current key header, `x-api-key` still accepted,
  `ANTHROPIC_API_KEY` as the SDK environment variable.
- [Versions](https://platform.claude.com/docs/en/api/versioning) and
  [Migrating to 2026-10-21](https://platform.claude.com/docs/en/api/migration-2026-10-21):
  current version pin; `temperature`, `top_p`, and `top_k` rejected.
- [Errors](https://platform.claude.com/docs/en/api/errors): status and error
  type table, 128 MB Messages request limit, 504 guidance toward streaming,
  529 overload.
- [Using the Messages API](https://platform.claude.com/docs/en/build-with-claude/working-with-messages):
  alternating turns, and the model-specific mid-conversation system role.
- [Define tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools):
  tool-name regex and `input_schema`.
- [Handle tool calls](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls):
  multiple `tool_use` blocks, and `tool_result` placement.
- [Stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons):
  `end_turn`, `max_tokens`, `stop_sequence`, `tool_use`, `pause_turn`,
  `refusal`, `model_context_window_exceeded`.
- [Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking)
  and [Thinking in tool workflows](https://platform.claude.com/docs/en/build-with-claude/thinking-tool-workflows):
  default thinking on current models, and verbatim content replay.
- [Structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs):
  `output_config.format` with `type: "json_schema"`.
- [OpenAI SDK compatibility](https://platform.claude.com/docs/en/cli-sdks-libraries/libraries/openai-sdk):
  the compatibility layer this design does not use.
