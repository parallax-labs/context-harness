# SPEC-0016: Model Runtime Spec

**Status:** Authoritative
**Date:** 2026-10-02
**Related:** [DESIGN-0010](../design/0010-local-agent-runtime.md), [execution plan](../design/0011-local-agent-runtime-execution-plan.md), [agent resources](0015-agent-resources.md), [ADR-0026](../adr/0026-native-ollama-local-generation.md)

## Scope

The application library SHALL provide a model interface independent of the agent
loop. This contract supports complete, non-streaming calls through a deterministic
fake, OpenAI Responses and native local Ollama adapters. It does not add CLI execution commands,
tool execution, automatic retries, streaming transport or checkpoint resume.
CLI orchestration and restricted context-tool execution are now defined by
[SPEC-0017](0017-local-agent-execution.md); streaming and resume remain later work.

## Neutral types and provider contract

`ModelProvider::generate` SHALL accept a `ModelRequest` and return a `ModelResponse`
or a categorized `ModelError`. Providers SHALL be Send + Sync. The selected
provider instance binds its provider model and credentials; a request cannot
change them.

Requests SHALL carry system/user messages, assistant text and tool calls,
tool results keyed by call ID, tool definitions with JSON parameter schemas,
an optional named output schema, and an optional positive output-token limit.
The request SHALL contain at least one message. Tool names SHALL be nonempty and
unique and parameter schemas SHALL be JSON objects.

Conversation validation SHALL reject duplicate historical call IDs, unmatched
or repeated tool results, and subsequent messages/model calls while tool results
are outstanding. Parallel calls are represented in one assistant message with
one result for each call before the next turn.

Responses SHALL contain text, tool calls, optional usage, finish reason, optional
parsed structured output, and optional provider continuation. Finish reasons are
`completed`, `tool_calls`, `length`, `refusal`, and `content_filter`. Only
`tool_calls` responses SHALL expose executable calls. Call IDs SHALL be nonempty,
unique and not reused from request history. Returned tools SHALL be declared in
the current request and their arguments SHALL be JSON objects.

A completed structured-output response SHALL contain valid JSON text matching
its parsed `structured_output` value. Full JSON Schema enforcement is delegated
to the production provider; the client does not implement a JSON Schema validator.
Tool parameter schemas are passed through, so the later tool runtime must still
validate arguments before dispatch.

Usage SHALL distinguish input, output and total token counts. Missing usage SHALL
remain absent rather than being reported as zero. Provider continuation SHALL be
serializable adapter-owned state; core code must retain it without interpreting
its contents. Its Debug representation SHALL omit its items.

## Registry and fake model

`ModelRegistry` SHALL bind unique aliases to provider/model identities and
provider implementations. Unknown aliases and duplicate registrations SHALL fail.
Custom Rust providers MAY register through the same interface.

`from_config` SHALL support `openai`, `fake` and `ollama` definitions from
`[models.*]` and reject unsupported providers. All built-ins SHALL use the same
trusted `ModelProviderCatalog` factory path. Provider names are case-sensitive;
duplicate registration SHALL fail without replacement. Registry construction SHALL
NOT call providers or read credential values. OpenAI credentials SHALL be read when a
call is made, using `api_key_env` or `OPENAI_API_KEY` when omitted. Ollama SHALL NOT
accept or resolve a credential reference.

`ModelDefinition` SHALL accept the common `provider` and `model` fields plus optional
`api_key_env`, `base_url` and `timeout_seconds` fields. OpenAI and fake definitions
SHALL reject `base_url` and `timeout_seconds`; Ollama definitions SHALL reject
`api_key_env`. Unknown fields SHALL remain errors.
Static validation SHALL parse and validate provider-specific fields without network
access, credential resolution, database access, model invocation or process startup.

Provider implementation metadata SHALL use stable non-secret identifiers. The Ollama
adapter identity is `context-harness.ollama-chat` version `1`. The selected model
definition, including its canonical base URL and timeout, remains part of the
checkpoint binding under [SPEC-0019](0019-checkpoints-recovery-and-artifacts.md).

`FakeModel` SHALL consume a queue of explicit response/error outcomes, one per
valid invocation. Invalid requests SHALL NOT consume outcomes. Exhaustion SHALL
return `script_exhausted`, never silently invent another answer. The configured
`fake` convenience provider SHALL contain one clearly labelled synthetic final
answer per registry construction. Tests needing multiple turns SHALL register a
scripted instance.

## OpenAI adapter

The adapter SHALL POST to `https://api.openai.com/v1/responses`, with the configured
model, `store: false`, `stream: false`, and the complete supplied history.
No hosted conversation or `previous_response_id` SHALL be required. It requests
`reasoning.encrypted_content` and SHALL preserve supported response output items
in their original order for the next turn.

Supported output items are assistant messages (text/refusal), function calls and
reasoning items. Unsupported output items, malformed completed calls, unexpected
roles and unknown response statuses SHALL fail explicitly. Truncated function
arguments in incomplete responses SHALL NOT be parsed as executable calls; the
adapter SHALL preserve the `length` or `content_filter` finish reason and usage.
Incomplete/refused responses SHALL expose neither calls nor continuation state.

On replay, the continuation provider and its decoded assistant text/calls SHALL
match the neutral assistant message. Altered or foreign continuation SHALL fail
before a network request. Tool results SHALL map to `function_call_output` using
the original call ID.

Tool names SHALL map to deterministic 64-character SHA-256 hex names on the wire,
then map back to the declared registry names. This accommodates dotted names
without ambiguous character replacement; descriptions include original names.
Function schemas SHALL use `strict: false` to preserve existing optional argument
semantics. Structured output SHALL use `text.format` with `json_schema` and
`strict: true`; callers supply a provider-compatible schema.

The HTTP client SHALL use a 60-second request timeout and 10-second connect timeout,
reject redirects, and cap response bodies at 4 MiB. It SHALL NOT automatically
retry failed calls. Error bodies SHALL NOT be logged or propagated. Diagnostics
SHALL expose only error category and optional HTTP status. Authentication,
rate-limit, service-unavailable, timeout, transport and malformed-response errors
remain distinguishable. Missing/empty credentials SHALL fail before sending.

## Ollama adapter

An `ollama` model definition SHALL have this shape:

```toml
[models.local]
provider = "ollama"
model = "qwen3"
base_url = "http://127.0.0.1:11434"
timeout_seconds = 120
```

`base_url` defaults to `http://127.0.0.1:11434`. It SHALL be an `http` URL containing
only a literal IPv4 or IPv6 loopback host and a nonzero explicit or scheme-default
port. User information, non-root paths, queries and fragments SHALL fail static
validation. Hostnames, including `localhost`, SHALL fail rather than trigger DNS.
HTTPS, Unix sockets and non-loopback addresses are unsupported. The adapter SHALL
append `/api/chat` itself and SHALL reject every redirect.

`timeout_seconds` defaults to 120 and SHALL be in `1..=3600`. The connect timeout is
fixed at 10 seconds. Serialized request and response bodies SHALL each be limited to
4 MiB. The adapter SHALL make one request with no automatic retry and no authorization
header. It SHALL set `stream: false` and `think: false`; it SHALL NOT set `keep_alive`
or call discovery, readiness, pull, create or delete endpoints.

System and user messages SHALL map to native messages with the same role and content.
Assistant text and tool calls SHALL replay as one assistant message. Each tool result
SHALL map to a `tool` message whose `tool_name` is recovered from its validated call
ID and whose content is the neutral result string. The adapter SHALL preserve message
order and SHALL reject a history it cannot map unambiguously.

Neutral tool names SHALL map to the same deterministic 64-character SHA-256 wire
names used by the OpenAI adapter, with the original name included in the description.
Response wire names SHALL map back only to tools declared in the request. Because the
native response does not supply call IDs, returned calls SHALL receive deterministic
IDs of the form `ollama-call-<assistant-ordinal>-<call-index>`, where both numbers are
zero-based in the complete neutral conversation. IDs SHALL be stable for the same
request/response and SHALL not collide with earlier call IDs.

Tool definitions SHALL use native function tools with the supplied object parameter
schemas. A named output schema SHALL map its schema object to `format`. When both tools
and an output schema are requested, the adapter SHALL send both rather than silently
drop either. `max_output_tokens` SHALL map to `options.num_predict`. A server or model
rejection SHALL remain an error; the adapter SHALL NOT retry without the requested
feature or limit.

A successful response SHALL require `done: true`, an assistant message and a nonempty
reported model name. Nonempty native tool calls produce the neutral
`tool_calls` finish reason. With no calls, native `done_reason: "length"` produces
`length`; absent, empty or `stop` produces `completed`; every other reason is an
invalid response. The native API has no neutral refusal or content-filter signal, so
the adapter SHALL NOT infer either from generated text.

For a completed structured-output request, the adapter SHALL parse message content as
JSON and expose the parsed value; the existing neutral response validation remains
authoritative. If both `prompt_eval_count` and `eval_count` are present, usage SHALL
set input, output and their checked sum as total tokens. If either count is absent,
usage SHALL remain absent. Ollama responses SHALL not create provider continuation.

The model error vocabulary SHALL include `model_unavailable` and
`unsupported_capability`. Ollama HTTP 404 maps to `model_unavailable`, 429 to
`rate_limited`, and 5xx to `unavailable`. Connection failure maps to `unavailable`, a
deadline to `timeout`, malformed success or error JSON to `invalid_response`, and
other transport failures to `transport`. A valid 400/422 Ollama error identifying an
unsupported capability maps to `unsupported_capability` only when its lowercased
`error` value contains `does not support tools`, or contains both `does not support`
and either `format` or `structured output`; other 400/422 responses map to
`invalid_request`. A request exceeding its body limit maps to `invalid_request`; a
success response exceeding its limit maps to `invalid_response`. Error bodies SHALL
be bounded, parsed only for classification, and never persisted, logged or returned.
Runtime cancellation SHALL drop the in-flight HTTP future and retain the existing
`run.cancelled` behavior; it SHALL not be recorded as a provider failure.

Static model inspection SHALL expose the effective canonical base URL, timeout and
local-only policy without contacting the endpoint. Readiness/model installation is
not part of static validation or this initial CLI contract. An opt-in live smoke test
MAY call `/api/chat` for an already installed model, but SHALL NOT pull a model.

## SQLite lifecycle recording

`generate_recorded` SHALL verify that the run belongs to the store's workspace
and its model alias matches the requested alias. It SHALL append `model.requested`
before contacting the provider; if this append fails, the model SHALL NOT run.
Terminal runs reject this append through the existing store API.

A UUID `call_id` SHALL correlate each requested event with `model.responded` or
`model.failed`. The requested payload contains model alias, provider/model names,
message/tool counts and a structured-output flag. Responses contain finish reason,
usage and tool-call count. Failures contain only categorized errors and status.
These payloads SHALL omit prompt text, tool arguments/results, output text,
continuation items, HTTP bodies/headers and credential values.

The API SHALL return the model response only after the response event is persisted.
A persistence failure after a provider call SHALL propagate without retrying the
call. Model failure SHALL NOT itself mark the run terminal: the future agent loop
owns run completion. Dropping/cancelling the call can leave an unmatched requested
event. Reconciliation and safe retry after interruption remain part of resume.

Metadata events are not complete conversation snapshots. Callers must retain
response messages for subsequent requests; durable conversation checkpoints and
payload retention policy belong to the runtime/checkpoint slice.

## Validation and references

Local HTTP fixtures cover request serialization, tool-name mapping, continuation
replay, structured output, refusal, truncation, token usage, response-size limits,
authentication/rate-limit failures, missing credentials and timeouts. File-backed
SQLite tests cover correlated lifecycle events, workspace/model bindings,
terminal runs, failed event writes and absence of prompt/output text in events.
No live or paid provider call is required for these tests.

Ollama fixtures additionally cover loopback URL validation, redirect rejection,
complete multi-turn tool replay, deterministic call IDs, simultaneous tools and
structured output, output-token limits, missing usage, model unavailable,
unsupported capability, endpoint unavailable, malformed responses, request/response
body limits and cancellation. Static list/show/validate tests SHALL pass while the
fixture endpoint is stopped. A network-observed test SHALL prove that the adapter
contacts only the configured loopback origin and never attempts a fallback request.

The adapter contract was checked against official documentation on 2026-09-25:
[Responses migration and stateless continuation](https://developers.openai.com/api/docs/guides/migrate-to-responses),
[function calling and strict schemas](https://developers.openai.com/api/docs/guides/function-calling).

The Ollama contract was checked against official documentation on 2026-10-02:
[native chat](https://docs.ollama.com/api/chat),
[tool calling](https://docs.ollama.com/capabilities/tool-calling),
[structured outputs](https://docs.ollama.com/capabilities/structured-outputs), and
[API errors](https://docs.ollama.com/api/errors).
