# ADR-0007: Trait-Based Extension System

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness needs a pluggable architecture for three kinds of extensions:

- **Connectors** — ingest data from external sources (filesystem, Git, S3,
  APIs, databases)
- **Tools** — expose capabilities to MCP clients (search, get, custom actions)
- **Agents** — define reusable AI personas with scoped tools and dynamic
  context injection

Extensions must be implementable in both Rust (for compiled harness binaries)
and Lua (for runtime scripting without recompilation). The system needs a
uniform discovery, registration, and invocation model regardless of the
implementation language.

## Decision

Define three core Rust **traits** with corresponding **registries**:

| Trait | Key Methods | Registry |
|-------|-------------|----------|
| `Connector` | `name()`, `description()`, `scan() → Vec<SourceItem>` | `ConnectorRegistry` |
| `Tool` | `name()`, `description()`, `parameters_schema()`, `execute(params, ctx)` | `ToolRegistry` |
| `Agent` | `name()`, `description()`, `tools()`, `arguments()`, `resolve(args, ctx)` | `AgentRegistry` |

A shared `ToolContext` struct bridges extensions to core functions:

- `ctx.search(query, options)` — search the knowledge base
- `ctx.get(id)` — retrieve a document by UUID
- `ctx.sources()` — list connector health

Registries are built at startup from configuration and passed as shared
state (`Arc`) to the HTTP server and MCP bridge. Both Rust-native and
Lua-scripted implementations register through the same trait interface
via adapter structs (`LuaConnectorAdapter`, `LuaToolAdapter`,
`LuaAgentAdapter`).

Modules:

- `src/traits.rs` — trait definitions, `ToolContext`, registries
- `src/agents.rs` — `Agent` trait, `AgentPrompt`, `AgentRegistry`, `TomlAgent`
- `src/connector_script.rs`, `src/tool_script.rs`, `src/agent_script.rs` —
  Lua adapters

## Alternatives Considered

**Dynamic library plugins (`.so`/`.dylib`).** Load extensions at runtime via
`dlopen`. This allows any language that compiles to a shared library, but
introduces ABI stability requirements, unsafe FFI boundaries, and
platform-specific packaging. Rust has no stable ABI, so plugin and host
must be compiled with the same toolchain version. Too fragile for a
distributed tool.

**WASM plugins.** Run extensions in a sandboxed WebAssembly runtime
(wasmtime, wasmer). Good isolation and portability, but adds significant
binary size, has ecosystem immaturity for async I/O and HTTP in WASI, and
introduces a compilation step for extension authors. The overhead is not
justified when Lua scripting covers the non-Rust extension use case well.

**Configuration-only (no code extensions).** Define connectors and tools
purely via TOML configuration (URL templates, header maps, etc.). This
works for simple HTTP connectors but cannot handle parsing logic, pagination,
authentication flows, or conditional behavior. A scripting layer is needed.

**gRPC / subprocess protocol.** Extensions as separate processes
communicating via gRPC or stdin/stdout. Adds operational complexity
(managing child processes, serialization overhead) and makes the tool
harder to distribute as a single binary.

## Consequences

- Uniform API: connectors, tools, and agents have consistent patterns
  for discovery (`list`), metadata (`name`, `description`), and invocation
  (`scan`, `execute`, `resolve`).
- Rust and Lua implementations are interchangeable at the registry level.
  The HTTP server and MCP bridge do not know or care about the implementation
  language.
- Adding a new extension type (e.g., "processors" or "validators") follows
  the established pattern: define a trait, create a registry, add Lua adapter
  support.
- The `ToolContext` bridge ensures extensions have controlled access to core
  functionality without exposing internal implementation details.
- Compiled Rust extensions have full access to the async runtime and can
  perform complex operations. Lua extensions run on blocking threads via
  `spawn_blocking` with instruction-count timeouts.
- The trait-based design enables custom harness binaries (via `examples/`)
  that compose different sets of connectors and tools for specific use cases.
