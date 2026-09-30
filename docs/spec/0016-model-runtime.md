# SPEC-0016: Model Runtime Spec

**Status:** Authoritative  
**Date:** 2026-09-25  
**Related:** [DESIGN-0010](../design/0010-local-agent-runtime.md), [execution plan](../design/0011-local-agent-runtime-execution-plan.md), [agent resources](0015-agent-resources.md)

## Scope

The application library SHALL provide a model interface independent of the agent
loop. This slice supports complete, non-streaming calls through a deterministic
fake and an OpenAI Responses adapter. It does not add CLI execution commands,
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

`from_config` SHALL support `openai` and `fake` definitions from `[models.*]` and
reject unsupported providers. Registry construction SHALL NOT call providers or
read credential values. OpenAI credentials SHALL be read when a call is made,
using `api_key_env` or `OPENAI_API_KEY` when omitted.

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

The adapter contract was checked against official documentation on 2026-09-25:
[Responses migration and stateless continuation](https://developers.openai.com/api/docs/guides/migrate-to-responses),
[function calling and strict schemas](https://developers.openai.com/api/docs/guides/function-calling).
