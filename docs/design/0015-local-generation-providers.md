# DESIGN-0015: Local Generation Providers

**Status:** Planning
**Date:** 2026-10-02
**Author:** Context Harness contributors
**Related:** [PRD-0015](../prd/0015-local-generation-providers.md), [DESIGN-0010](0010-local-agent-runtime.md), [ADR-0026](../adr/0026-native-ollama-local-generation.md), [SPEC-0016](../spec/0016-model-runtime.md), [SPEC-0017](../spec/0017-local-agent-execution.md)

## Context

`agent_model::ModelProvider` is already provider-neutral, and `RecordingModel` binds
provider calls to durable run history. Provider construction is not neutral:
`ModelRegistry::from_config` matches configured provider strings and directly creates
OpenAI or fake implementations. `ModelDefinition` currently exposes only `provider`,
`model`, and `api_key_env`.

Local embeddings are a separate ingestion capability. They do not run agent model
turns. This design adds local generation without coupling it to background execution
or an application domain.

## Proposal

### Provider catalog

Introduce a trusted `ModelProviderCatalog` (name subject to spec) whose factories:

- expose a stable provider implementation identity and version;
- validate provider-specific configuration without side effects;
- build a provider only for runtime execution;
- declare supported capabilities and configuration keys;
- reject collisions deterministically.

Built-in OpenAI, fake, and the first local adapter use the same catalog path. A host
may register compiled factories explicitly; declarative resources cannot load code.
Model aliases still select a provider and model and do not grant network authority.

Provider-specific configuration should be typed per factory. The serialized model
definition may retain a flattened table for forward compatibility, but each selected
factory rejects unknown or invalid keys. Secret fields remain environment-variable
references and are excluded from public identity values.

### Initial Ollama adapter

Recommend Ollama's native non-streaming chat API for the first adapter. It exposes
installed model identity and tool calls directly. An OpenAI-compatible local adapter
can follow without defining “local” as wire compatibility with OpenAI.

Illustrative configuration only:

```toml
[models.local]
provider = "ollama"
model = "installed-model-name"
base_url = "http://127.0.0.1:11434"
timeout_seconds = 120
```

Static validation checks syntax, limits, and endpoint policy but does not connect to
Ollama or claim the model is installed. Readiness is deferred from the initial
adapter.

The adapter converts existing `ModelMessage` variants and `ModelTool` schemas to the
provider protocol. It reconstructs assistant tool calls and tool results on subsequent
turns, maps text and finish reason, preserves reported usage, and validates every
response against the original `ModelRequest`. Missing usage remains `None`.

The HTTP client bounds request/response size, applies connect and total timeouts,
propagates cancellation, and disables redirects. The resolved destination must be a
literal loopback HTTP origin. No provider fallback occurs. The adapter never invokes a
model-pull endpoint.

**Contract status (2026-10-02): Resolved.** [ADR-0026](../adr/0026-native-ollama-local-generation.md)
selects the native API and a literal-loopback HTTP boundary. The revised
[SPEC-0016](../spec/0016-model-runtime.md) defines typed configuration, protocol
mapping, capability failures, identity, body and time limits, cancellation and static
inspection. Readiness remains outside the first adapter.

### Capability and identity behavior

Provider identity includes implementation ID/version plus sanitized effective
configuration and selected model. Changing relevant identity invalidates conservative
resume exactly as other model changes do. Readiness and model-quality evaluation are
not static validation.

If the server/model cannot satisfy a requested feature—tools, structured output, or
output bounds—the provider fails before or during the call with a classified error;
it does not silently drop the feature. A small evaluation fixture records protocol
success separately from answer quality.

### Public host integration

Export catalog/factory registration through the crate's supported API. The CLI builds
the standard catalog and embedding applications may add trusted factories before
constructing `ModelRegistry`. Registration itself makes no network call. This is the
model-specific portion of the host API; general background host lifecycle belongs to
DESIGN-0016.

## Alternatives Considered

- **Reuse the OpenAI adapter with a custom base URL:** expedient but does not model
  Ollama-specific capabilities, installed models, or locality. Keep as a later adapter.
- **Shell out to an Ollama CLI:** complicates cancellation, structured errors, body
  limits, and tool-call continuity. Prefer the HTTP protocol.
- **Support Ollama and llama.cpp together:** multiplies protocol work before one local
  end-to-end path is proven. Start with one adapter and a reusable catalog.
- **Build local inference into the runtime loop:** would violate provider neutrality
  and duplicate validation/recording. Keep it behind `ModelProvider`.

## Implementation Plan

1. Extract provider factory/catalog construction while preserving OpenAI/fake tests.
2. Define typed provider-specific configuration, redaction, and identity behavior.
3. Implement Ollama request/response mapping with fixture HTTP tests.
4. Add local-only endpoint enforcement, body bounds, timeouts, and cancellation.
5. Publish an opt-in live smoke test and small tool-calling evaluation fixture.

## Acceptance Criteria

- An ordinary agent completes a multi-turn local tool loop through `AgentRuntime`.
- Static inspection works offline and is side-effect free.
- Local-only tests reject non-loopback destinations and redirect escapes.
- Provider unavailability never selects another alias or provider.
- Malformed/oversized responses and unsupported features fail safely.
- A custom compiled provider registers without a new runtime match branch.
- OpenAI/fake behavior and checkpoint identity tests remain compatible.

## Deferred questions

1. Should a later provider support enrolled LAN/HTTPS endpoints or Unix sockets?
2. Which installed models form the non-normative live quality evaluation set?
3. Does a later explicit readiness command justify a stable capability-report schema?
