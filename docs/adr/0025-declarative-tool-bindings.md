# ADR-0025: Declarative Bindings over the Existing Tool Registry

**Status:** Accepted
**Date:** 2026-09-30

## Context

[PRD-0013](../prd/0013-declarative-tool-bindings.md) requires reusable tool composition.
The runtime uses ToolRegistry but constructs fixed local tools and validates them
by name. Server extension support does not yet provide an equivalent executable
runtime path. Domain applications would otherwise expand core dispatch indefinitely.

## Decision

The proposed decision is to resolve declarative tool resources into adapters using
the existing Tool/ToolRegistry contract. Implementations provide executable behavior
and validation; bindings provide public names, fixed configuration and enforceable
restrictions. Agent resources select resolved tools. The runtime performs generic
policy checks and invocation lifecycle operations, without domain-name dispatch.

Authority originates with the host and trusted implementation descriptors. Binding
and agent declarations can narrow it, never manufacture privileges or sandbox an
unrestricted implementation. Unsupported restrictions cause binding rejection.
Domain-specific behavior lives in extensions/resources; general execution and
policy machinery lives in Context Harness.

The concrete contract is [SPEC-0023](../spec/0023-declarative-tool-bindings.md).
Acceptance authorizes implementation against that specification; it does not assert
that the binding layer already exists. ADR-0024 remains unchanged.

## Alternatives Considered

- Add domain-specific built-ins repeatedly: initially fast, but each new application
  requires runtime changes and duplicates validation/policy decisions.
- Create a second tool execution abstraction: splits approvals, cancellation and
  history from existing integrations without a demonstrated need.
- Treat all capability declarations as trusted: permits configuration to mislabel
  unrestricted implementations and is not an enforceable permission model.
- Expose only external MCP processes: useful extension mechanism, but cannot alone
  enforce narrow filesystem scopes over an unsandboxed remote/process backend.

## Consequences

- Tool construction, validation, provenance and authority need reusable contracts.
- Arbitrary Lua/Rust/MCP code remains explicitly trusted or denied; annotations
  do not replace sandboxing. Existing conservative defaults remain available.
- Resource/schema resolution adds complexity, but applications do not need core
  name-based branches. Existing built-in names require compatibility adapters.
- Application features, including wiki management, wait for the foundation's tests.

## References

- [DESIGN-0013](../design/0013-declarative-tool-bindings.md)
- [ADR-0024](0024-local-agent-runtime.md)
- [SPEC-0023](../spec/0023-declarative-tool-bindings.md)
