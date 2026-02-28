# PRD-0006: Workspace Refactor and Library Publishing

**Status:** Planned
**Date:** 2026-02-27
**Author:** pjones

## Problem Statement

Context Harness is a single Rust crate that bundles everything: models,
chunking, search algorithm, SQLite storage, connectors, server, CLI, Lua
runtime, and MCP bridge. This works for the CLI/server use case but
creates problems for three emerging needs:

1. **Library consumers.** A Tauri desktop app or custom binary that wants
   to use Context Harness search and chunking must pull in the entire
   dependency tree (tokio, sqlx, axum, mlua, clap). There is no way to
   depend on just the core logic.

2. **WASM target.** Running chunking, embedding (tract), and search in
   the browser requires code that compiles to `wasm32-unknown-unknown`.
   Today, core logic is entangled with SQLite and tokio, which do not
   compile to WASM.

3. **Pluggable storage.** Search and ingest talk directly to SQLite via
   sqlx. Supporting alternative backends (Qdrant, Weaviate, in-memory
   for WASM) requires abstracting storage behind a trait.

4. **Publishing.** The crate cannot be published to crates.io as a
   library in its current form because consumers would inherit the full
   application dependency tree.

## Target Users

1. **Library consumers** building custom applications (Tauri desktop app,
   custom binaries) who want Context Harness search and chunking without
   the full application stack.
2. **WASM consumers** (future) who need core logic that compiles to
   `wasm32-unknown-unknown`.
3. **Teams with existing vector infrastructure** who want to use their
   Qdrant or Weaviate cluster instead of SQLite for embedding storage.
4. **Existing CLI/MCP users** who should see zero changes -- the refactor
   is invisible to them.

## Goals

1. Split the repository into a Cargo workspace with two crates:
   `context-harness-core` (lightweight, WASM-safe) and `context-harness`
   (full application).
2. Define a `Store` trait in core that abstracts all storage operations,
   enabling pluggable backends.
3. Deliver two Store implementations in Phase 0: `InMemoryStore` (in
   core, for WASM and testing) and `SqliteStore` (in the app, preserving
   current behavior).
4. Preserve the public API of `context-harness` exactly -- all modules,
   re-exports, function signatures, CLI behavior, and MCP endpoints
   remain unchanged.
5. Publish `context-harness-core` to crates.io as a lightweight library
   that external consumers can depend on.

## Non-Goals

- Implementing WASM support (that is PRD-0007).
- Building the Tauri desktop app (that is PRD-0008).
- Implementing Qdrant or Weaviate backends (future work after the Store
  trait is in place).
- Changing any CLI commands, MCP endpoints, config file format, or
  search behavior.

## User Stories

**US-1: Tauri app depends on core.**
A developer building a Tauri desktop app adds `context-harness-core` to
their `Cargo.toml`. They get access to chunking, search algorithm, Store
trait, EmbeddingProvider trait, and all data models -- without pulling in
tokio, sqlx, axum, or clap.

**US-2: Custom binary with SqliteStore.**
A power user builds a custom binary that depends on `context-harness`.
They use `SqliteStore` (which implements the Store trait) and the existing
`run_server_with_extensions()`. Their code requires zero changes from
today.

**US-3: Existing CLI user sees no difference.**
A user who has been running `ctx sync all && ctx search "auth"` upgrades
to the workspace version. Everything works identically. Same commands,
same output, same database format.

**US-4: In-memory store for testing.**
A contributor writes tests using `InMemoryStore` from core instead of
setting up SQLite. Tests run faster and have no filesystem side effects.

## Requirements

### Workspace Structure

1. The root `Cargo.toml` SHALL define a workspace with members
   `crates/context-harness-core` and `crates/context-harness`.
2. `context-harness-core` SHALL NOT depend on tokio, sqlx, `std::fs`,
   reqwest, axum, mlua, or clap.
3. `context-harness-core` SHALL compile to `wasm32-unknown-unknown`.

### What Moves to Core

4. Core SHALL contain: data models (`Document`, `Chunk`, `SourceItem`,
   `SearchResultItem`, `ScoreExplanation`, `ChunkCandidate`), chunking
   (`chunk_text`), Store trait, in-memory Store implementation,
   `EmbeddingProvider` trait + tract implementation, hybrid search
   algorithm (normalize, merge, aggregate, sort), and vector utilities
   (`cosine_similarity`).

### What Stays in the App

5. The app SHALL contain: config, db, migrate, ingest, connectors (fs,
   git, S3, script), Lua runtime, tool scripts, agent scripts, server,
   MCP bridge, traits (Connector, Tool, ToolContext, registries),
   fastembed/OpenAI/Ollama embedding providers, SqliteStore, and CLI.

### Store Trait

6. The Store trait SHALL define operations: upsert document, replace
   chunks (with optional vectors), upsert embedding, get document
   (full), get document metadata, keyword search, and vector search.
7. The Store trait SHALL use `async-trait` for async operations.
8. Core's search function SHALL accept retrieval tuning parameters
   directly (not via the full Config struct).

### API Preservation

9. `context-harness` SHALL re-export all types and functions at the
   same module paths as today. No breaking change for existing library
   consumers.
10. CLI commands, MCP endpoints, and config file format SHALL remain
    unchanged.

### Path and Build Updates

11. `flake.nix`, CI workflows, and all path references SHALL be updated
    for the new workspace layout.

## Success Criteria

- `cargo build -p context-harness-core --target wasm32-unknown-unknown`
  succeeds.
- `cargo test` at the workspace root passes all existing tests.
- `cargo publish -p context-harness-core` succeeds (dry-run).
- The public API of `context-harness` (checked via `cargo doc`) has no
  removals compared to the pre-refactor version.
- CLI and MCP integration tests produce identical results.

## Dependencies and Risks

- **Migration complexity:** Moving code between crates while preserving
  the public API requires careful re-exporting. The spec defines exact
  migration steps.
- **Build time:** Two crates may increase incremental build time slightly.
  Workspace-level caching mitigates this.
- **Nix flake updates:** The flake must be updated to build the workspace
  and produce the same output binaries. ADR-0016 is affected.

## Related Documents

- **ADRs:** [0018](../adr/0018-store-abstraction-and-workspace-split.md)
- **Specs:** [WORKSPACE_REFACTOR_SPEC.md](../WORKSPACE_REFACTOR_SPEC.md)
- **PRDs:** [PRD-0001](0001-core-context-engine.md) (core engine being
  split), [PRD-0004](0004-local-first-embeddings.md) (tract moves to
  core), [PRD-0007](0007-wasm-client-and-browser-rag.md) (depends on
  this refactor), [PRD-0008](0008-tauri-desktop-application.md) (depends
  on this refactor)
