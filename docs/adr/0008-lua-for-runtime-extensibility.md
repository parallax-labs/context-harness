# ADR-0008: Lua 5.4 for Runtime Extensibility

**Status:** Accepted
**Date:** Retroactive

## Context

While the trait-based extension system (see
[ADR-0007](0007-trait-based-extension-system.md)) allows Rust-native
extensions, most users should not need to compile Rust to add a connector,
tool, or agent. The system needs a scripting layer that:

- Allows writing connectors for arbitrary APIs (Jira, Confluence, Notion, etc.)
- Allows writing custom MCP tools (create tickets, post messages, etc.)
- Allows writing dynamic agents with context injection logic
- Is safe to run — scripts must not be able to access arbitrary filesystem
  paths, spawn processes, or consume unbounded resources
- Adds minimal binary size overhead
- Has a simple, well-known syntax accessible to non-Rust developers

## Decision

Use **Lua 5.4** via the `mlua` crate with the **vendored** feature flag
(Lua is compiled into the binary, no system dependency).

### Sandboxing

The following standard Lua libraries are **removed**:

- `os` — no process spawning, environment mutation, or clock access
- `io` — no arbitrary file I/O
- `loadfile`, `dofile` — no loading arbitrary Lua files
- `debug` — no introspection of the VM internals

### Host APIs

Scripts access system capabilities through curated host APIs:

| API | Purpose | Restrictions |
|-----|---------|-------------|
| `http` | HTTP requests (GET, POST, etc.) | None (scripts need network for APIs) |
| `json` | JSON encode/decode | None |
| `fs` | Read files, list directories | Sandboxed to the script's own directory |
| `env` | Read environment variables | Read-only |
| `log` | Structured logging | None |
| `base64` | Base64 encode/decode | None |
| `crypto` | SHA-256 hashing, HMAC | None |
| `sleep` | Async sleep | None |

### Execution Model

- Lua scripts run on **blocking threads** via `tokio::task::spawn_blocking`
  to avoid blocking the async runtime.
- An **instruction-count timeout** checks every 10,000 Lua instructions
  and aborts scripts that exceed the configured timeout.
- Each script invocation gets a fresh Lua state — no shared mutable state
  between calls.

### Script Interface

All three extension types follow the same pattern:

```lua
local ext = {}
ext.name = "my-extension"
ext.description = "What it does"
function ext.scan(config, context)    -- connectors
function ext.execute(params, context) -- tools
function ext.resolve(args, config, context) -- agents
return ext
```

## Alternatives Considered

**Embedded Python (PyO3).** Rich ecosystem and familiar to many developers,
but the Python runtime adds tens of megabytes to binary size, requires
managing a Python installation or bundling libpython, and has GIL contention
issues. Distribution complexity conflicts with the single-binary goal.

**Embedded JavaScript (Deno/V8, QuickJS).** V8 is extremely capable but
adds 20+ MB to binary size. QuickJS is lighter but has a smaller ecosystem.
Neither is as lightweight or embeddable as Lua. JavaScript's async model
also introduces complexity in the bridge layer.

**Rhai.** A Rust-native scripting language designed for embedding. Smaller
than Lua but has a much smaller user community, fewer learning resources,
and less mature tooling. Lua's decades of embedding history and extensive
documentation make it a safer choice.

**Starlark (Python-like, used by Bazel/Buck).** Deterministic and sandboxed
by design, but the Rust implementation (`starlark-rust`) is less mature and
the syntax, while Python-like, has surprising limitations (no classes, limited
standard library).

**WASM-based scripting.** Run extension scripts compiled to WASM. Good
sandboxing but adds a compilation step for extension authors and increases
binary size. The WASI ecosystem for async I/O and HTTP is not yet mature
enough for the required host APIs.

## Consequences

- Users can write connectors, tools, and agents in Lua without any Rust
  toolchain. This dramatically lowers the barrier to extending Context Harness.
- The vendored Lua 5.4 adds approximately 300 KB to the binary — negligible
  compared to the embedding model or SQLite.
- Sandboxing via library removal and scoped `fs` access provides meaningful
  isolation. Scripts cannot access files outside their directory, spawn
  processes, or mutate the environment.
- The instruction-count timeout prevents runaway scripts from consuming
  CPU indefinitely, protecting the server during `ctx serve mcp`.
- Lua's simplicity means the host API surface is small and auditable.
  Each new host function is an explicit security decision.
- `spawn_blocking` execution means Lua scripts do not interfere with the
  async HTTP server, but throughput is limited by the blocking thread pool
  size (tokio default: 512 threads).
- The shared Lua runtime module (`lua_runtime.rs`) is used by connectors,
  tools, and agents, ensuring consistent host API availability and
  sandboxing across all extension types.
