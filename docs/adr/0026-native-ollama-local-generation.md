# ADR-0026: Native Ollama API for Local Generation

**Status:** Accepted
**Date:** 2026-10-02

## Context

[PRD-0015](../prd/0015-local-generation-providers.md) requires an explicit local
generation provider with no cloud fallback, automatic model download or
side-effecting static validation. Ollama exposes both a native chat API and an
OpenAI-compatible API. Wire compatibility alone does not establish that an endpoint
is local, and reusing the OpenAI adapter would obscure Ollama-specific tool calls,
structured output, usage and model errors.

The provider-neutral runtime already owns conversation validation, tool execution,
recording and cancellation. The adapter therefore needs a narrow transport contract,
not a second agent loop or a provider CLI integration.

## Decision

The built-in `ollama` generation provider SHALL use Ollama's native, non-streaming
`POST /api/chat` API. It SHALL replay the complete provider-neutral conversation and
use the native `tools`, `format`, `options.num_predict` and usage fields.

The built-in provider is local-only by definition. Its endpoint SHALL be an `http`
origin whose host is a literal IPv4 or IPv6 loopback address. DNS names, Unix sockets,
HTTPS, credentials, URL paths, queries, fragments and redirects are excluded from the
initial contract. The default endpoint is `http://127.0.0.1:11434`. There is no
configuration switch that converts this provider into a remote or cloud adapter.

Configuration SHALL be typed and limited to the selected model, `base_url` and
`timeout_seconds`. Static validation SHALL validate these values without resolving
credentials, opening the run database or contacting Ollama. Runtime construction
SHALL NOT perform readiness or model-discovery calls. The adapter SHALL never invoke
pull, create or delete endpoints and SHALL never fall back to OpenAI or another model
alias.

No readiness CLI is part of the first adapter. Protocol conformance is covered by a
local fixture server; an opt-in live smoke test may verify an already installed model
without becoming static validation.

The authoritative behavioral contract is [SPEC-0016](../spec/0016-model-runtime.md).

## Alternatives Considered

- **Ollama's OpenAI-compatible API:** easier adapter reuse, but it hides native
  capability and error semantics and makes an OpenAI-shaped URL look like proof of
  locality.
- **Configurable remote Ollama endpoints:** useful for LAN or hosted deployments, but
  incompatible with the first slice's enforceable no-cloud boundary. A later provider
  or superseding ADR can add enrolled remote authority.
- **The Ollama CLI:** adds process startup, output parsing and weaker cancellation and
  body bounds without improving the native HTTP contract.
- **Unix sockets in the first slice:** retain a useful future extension, but require a
  separate transport and authority model that is unnecessary for loopback support.
- **Implicit readiness and model download:** convenient, but makes validation
  side-effecting and changes operator-owned model state.

## Consequences

- Locality is enforceable at the adapter boundary and does not depend on a hostname
  resolving as expected.
- Users running Ollama on another machine, behind HTTPS or through a Unix socket need
  a future explicitly authorized provider contract.
- Native messages require deterministic adapter-generated tool-call IDs because
  Ollama does not supply the neutral IDs used by Context Harness.
- Model capability failures remain visible; the adapter does not silently remove
  tools, structured-output schemas or output bounds.
- Fixture tests can exercise the full protocol without an installed model or network
  access outside the test process.

## References

- [PRD-0015](../prd/0015-local-generation-providers.md)
- [DESIGN-0015](../design/0015-local-generation-providers.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [SPEC-0016](../spec/0016-model-runtime.md)
- [Ollama chat API](https://docs.ollama.com/api/chat)
- [Ollama tool calling](https://docs.ollama.com/capabilities/tool-calling)
- [Ollama structured outputs](https://docs.ollama.com/capabilities/structured-outputs)
- [Ollama API errors](https://docs.ollama.com/api/errors)
