# PRD-0015: Local Generation Providers

**Status:** Draft
**Date:** 2026-10-02
**Author:** Context Harness contributors

## Problem Statement

Context Harness agents use a provider-neutral runtime contract, but the built-in
production adapter currently calls OpenAI. Users who want private, offline-capable,
or zero-marginal-call-cost agent execution cannot configure an installed local model
without writing a custom embedding host. Local embeddings do not solve generation:
the agent's multi-turn reasoning and tool calls still require a generation provider.

Context Harness needs a supported local generation path that preserves the same
bounded requests, tool validation, history, cancellation, and error classification as
other providers. Local execution must be explicit and must never imply a cloud
fallback or automatic model download.

## Target Users

- Developers running Context Harness agents on machines with local inference servers.
- Application authors who need local model execution through supported public APIs.
- Operators who require a clear no-cloud-fallback boundary for selected agents.
- Provider authors implementing additional generation adapters.

## Goals

1. Configure and run a tool-calling agent with an explicitly installed local model.
2. Preserve the existing provider-neutral model and agent-runtime contracts.
3. Validate and inspect local model aliases without contacting a provider or resolving
   secrets.
4. Fail visibly when the endpoint or model is unavailable, with no implicit provider
   fallback or model download.
5. Let trusted embedding hosts register additional provider implementations without
   changing runtime dispatch code.

## Non-Goals

- Choosing or downloading a model for the user.
- Guaranteeing that every model supports reliable tools or structured output.
- Replacing local embedding providers or changing embedding configuration.
- Routing one request dynamically across local and paid providers.
- Background task execution, scheduling, queues, or application-specific workflows.
- Treating an arbitrary OpenAI-compatible endpoint as proven local execution.

## User Stories

- I configure an installed Ollama model and run my existing agent without changing
  its tools or prompt.
- I inspect the effective endpoint policy and limits before making a model call.
- If Ollama or the selected model is unavailable, I receive a distinct local error
  and know that no paid provider was called.
- I run a fixture evaluation before trusting a local model with unattended tools.
- As an embedding host, I register a provider factory and reuse Context Harness's
  model validation, recording, and runtime loop.

## Requirements

| ID | Requirement |
|---|---|
| L1 | Model aliases support an explicit local generation provider and provider-specific validated settings. |
| L2 | The provider maps multi-turn messages, tool definitions, tool calls/results, output bounds, finish reasons, structured output when supported, cancellation, and usage when reported. |
| L3 | A local-only configuration rejects disallowed endpoints and redirects and never falls back to another provider. |
| L4 | Static model inspection and validation make no network request, resolve no credential value, download no model, and start no provider process. |
| L5 | Runtime errors distinguish invalid configuration, endpoint unavailable, model unavailable, malformed provider response, timeout, cancellation, and unsupported capability without persisting secret or raw response data. |
| L6 | The runtime never downloads model weights implicitly; live readiness checks are explicit operations. |
| L7 | Trusted hosts can register model provider factories through a documented public interface with deterministic collision and identity rules. |
| L8 | Existing OpenAI/fake aliases and provider-neutral agent behavior remain compatible. |

## Success Criteria

- A generic fixture agent completes a local `search -> get -> final answer` tool loop.
- Fixture tests cover malformed calls, unknown tools, unsupported finish states,
  oversized bodies, timeouts, cancellation, unavailable model, and redirect escape.
- Network-observed live smoke testing confirms that a local-only run contacts only
  its enrolled endpoint and makes no OpenAI request.
- Static list/show/validate succeeds with the local server stopped and exposes no
  secret values.
- An external fixture host registers a test provider without editing provider-name
  branches in the runtime.
- Existing provider and agent-runtime regression suites remain green.

## Dependencies and Risks

This work depends on SPEC-0016's provider-neutral types and SPEC-0017's bounded agent
loop. Local servers differ in message formats, tool-call semantics, structured output,
usage reporting, context limits, cancellation, and error bodies. “Loopback” constrains
network destination but cannot prove that a provider will not proxy elsewhere.
Model quality must be evaluated separately from protocol correctness.

## Open Questions

1. Should the first adapter use Ollama's native chat API or an OpenAI-compatible API?
2. Which endpoint schemes count as local-only: loopback HTTP, Unix sockets, or both?
3. How are provider-specific settings represented without turning model definitions
   into an unvalidated catch-all?
4. Which capability/readiness checks belong in the initial CLI?
5. How should structured output and tool calls interact when a model/server cannot
   reliably provide both?

These questions must be resolved before this PRD moves to Planned.

## Related Documents

- [DESIGN-0015](../design/0015-local-generation-providers.md): provider catalog,
  recommended first adapter, risks, and implementation slices.
- [DESIGN-0010](../design/0010-local-agent-runtime.md): provider-neutral runtime
  direction.
- [SPEC-0016](../spec/0016-model-runtime.md): current model request/response contract.
- [SPEC-0017](../spec/0017-local-agent-execution.md): bounded execution loop.
- [PRD-0016](0016-durable-background-agent-tasks.md): separate asynchronous task
  lifecycle that can consume any configured provider.
- Future ADR/spec updates are deferred until the design questions are resolved.
