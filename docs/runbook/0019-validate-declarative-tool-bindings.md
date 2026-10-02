# RUNBOOK-0019: Validate Declarative Tool Bindings

**Status:** Active  
**Last verified:** 2026-10-01  
**Applies to:** SPEC-0023 declarative tool resources and local agent runtime

## Purpose

Verify static discovery and inspection, scoped built-in composition, compiled and
Lua extensions, MCP aliases, durable binding identity, and compatibility behavior.
The procedure is local and deterministic; its MCP coverage uses the repository's
stdio fixture and does not access a third-party service.

## Prerequisites

- Run from the repository root.
- Use the Rust toolchain pinned by the repository.
- Python 3 must be available for the local MCP fixture.

## Static inspection checks

Run the binding-focused suite:

```sh
cargo test -p context-harness --test tool_bindings
```

This proves deterministic layering, explicit-config isolation, fixed-argument
projection, static MCP inspection, scoped readers, source-scoped retrieval, and
checkpoint identity changes. Static inspection must not create the configured
database or start an MCP process.

For a real workspace with `.ctx/tools/*.toml`, inspect the resolved resources:

```sh
cargo run -p context-harness -- tool bindings list --json
cargo run -p context-harness -- tool bindings validate --json
cargo run -p context-harness -- tool bindings show <public-name> --json
```

Check the reported scope, resource path, implementation and binding versions,
public schema, fixed arguments, restrictions, capabilities, and trust class.
Secret references must appear as `secret_ref` markers, never resolved values. An
MCP alias must report `remote_metadata: "unresolved"`; that is a successful static
declaration check, not a claim that the remote schema was verified.

## Runtime backend checks

Run each backend's focused acceptance suite:

```sh
cargo test -p context-harness --test compiled_tool_bindings
cargo test -p context-harness --test lua_tool_bindings
cargo test -p context-harness --test agent_mcp_runtime
```

Expected results:

- Compiled bindings pass generic validation, output bounds, cancellation, and host
  authority checks without public-name dispatch.
- Lua modules cannot evaluate before authorized preparation. Invalid arguments,
  execution failure, output limits, and cancellation are durable.
- MCP startup and calls retain process/external-side-effect policy. Fixed arguments
  are included in per-call approval, schema drift fails before model use, and MCP
  runs remain non-resumable.

## Full compatibility gate

Run formatting, linting, the complete regression suite, and diff hygiene:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All commands must exit zero. Ignored performance probes are expected and are not a
failure. Do not mark the feature verified from focused fixtures alone.

## Troubleshooting

- `unknown tool implementation`: register the trusted implementation in the host
  catalog, or correct the resource's implementation ID.
- `cannot enforce restriction`: remove the unsupported declaration or choose an
  implementation whose descriptor can enforce it.
- `MCP alias schema validation failed`: compare the authorized discovered schema
  with fixed arguments and the resource's expected public contract.
- Lua preparation denial: install an approval handler and grant every capability
  required by the privileged manifest; do not downgrade the manifest.
- Resume rejection after a binding change is expected. Inspect the original run
  history; do not replay uncertain or external-session work under a new identity.
