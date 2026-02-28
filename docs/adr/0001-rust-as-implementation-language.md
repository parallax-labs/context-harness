# ADR-0001: Rust as Implementation Language

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness is a local-first context ingestion and retrieval tool for AI
workflows. It needs to:

- Parse, chunk, and embed documents from multiple sources (filesystem, Git, S3,
  Lua scripts)
- Run an HTTP server hosting both REST and MCP endpoints
- Perform vector similarity search over potentially tens of thousands of chunks
- Ship as a single, self-contained binary with no runtime dependencies
- Run on Linux (x86_64, aarch64, musl), macOS (x86_64, aarch64), and Windows

The language choice affects binary size, startup time, memory footprint, build
complexity, and the ability to embed native libraries (SQLite, Lua, embedding
models) without requiring users to install system packages.

## Decision

Use **Rust** (edition 2021) with the **tokio** async runtime as the sole
implementation language.

Key framework choices that follow from this:

- `clap` (derive) for CLI parsing
- `sqlx` for async SQLite access
- `axum` for the HTTP server
- `mlua` (vendored) for Lua embedding
- `serde` / `serde_json` / `toml` for serialization

## Alternatives Considered

**Go.** Simpler concurrency model and fast compilation, but garbage collection
introduces latency spikes during embedding and search. The type system lacks
enums, generics (pre-1.18), and trait-based abstraction, making the extension
system harder to express safely. CGo is required for SQLite, which complicates
cross-compilation.

**Python.** Rapid prototyping and rich ML ecosystem, but poor performance for
brute-force vector search and chunking. Distribution requires users to manage
virtualenvs or use bundlers like PyInstaller, which produce large artifacts
and have platform-specific issues.

**TypeScript / Node.js.** Good ecosystem for HTTP servers, but high memory
overhead, large binary size (via pkg/nexe), and poor support for embedding
native libraries like Lua and SQLite without native addons.

## Consequences

- Single static binary on all platforms; no runtime dependencies for users.
- Compile times are longer than Go or TypeScript, partially mitigated by
  incremental compilation and `cargo-zigbuild` for cross-targets.
- The Rust ecosystem provides high-quality crates for every layer (sqlx, axum,
  mlua, fastembed, rmcp), reducing custom code.
- Contributors must know Rust, which has a steeper learning curve. This is
  offset by Lua scripting for connectors, tools, and agents — most extension
  authors never touch Rust.
- Memory safety and ownership semantics eliminate entire classes of bugs
  (use-after-free, data races) that would be critical in a tool handling
  user documents.
