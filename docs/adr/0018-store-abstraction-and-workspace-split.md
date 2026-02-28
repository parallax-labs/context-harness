# ADR-0018: Store Abstraction and Workspace Split

**Status:** Accepted
**Date:** 2026-02-27

## Context

Context Harness is a single Rust crate (`context-harness`) that contains
everything: models, chunking, embedding, search algorithm, SQLite storage,
connectors, server, CLI, and MCP bridge. This has worked well for rapid
development, but several forces now push toward a structural split:

1. **Library publishing.** External consumers (e.g. a native Tauri frontend)
   want to depend on Context Harness as a Rust library. Today, pulling in
   `context-harness` brings the entire dependency tree (tokio, sqlx, axum,
   mlua, clap, etc.), even if the consumer only needs search and chunking.

2. **Future WASM target.** The WASM Client Design (see Obsidian notes)
   calls for running chunking, embedding (tract), and search in the browser.
   Today, these are entangled with SQLite and tokio, which do not compile
   to `wasm32-unknown-unknown`.

3. **Pluggable storage backends.** Search and ingest talk directly to SQLite
   via `sqlx` (FTS5 for keywords, `chunk_vectors` for embeddings). To support
   alternative vector databases (Qdrant, Weaviate, etc.) or an in-memory
   store (for WASM), the storage access must be abstracted behind a trait.

4. **Custom binaries and integrations.** Users building custom harness
   binaries (via the trait-based extension system, ADR-0007) benefit from
   depending on a lighter core crate rather than the full application.

The question is how to restructure the codebase to address these forces
without breaking any existing contracts (public API, CLI, MCP, config).

## Decision

Refactor the repository into a **Cargo workspace** with two crates and
introduce a **Store trait** as the storage abstraction. The authoritative
specification is `docs/WORKSPACE_REFACTOR_SPEC.md`.

### Workspace layout

| Crate | Purpose | Targets |
|-------|---------|---------|
| `context-harness-core` | Shared, WASM-safe logic: models, chunking, Store trait, in-memory store, embedding trait + tract, search algorithm, vector utilities | native + wasm32 |
| `context-harness` | CLI, server, connectors, config, db, migrate, ingest, traits, tools, agents, MCP. Implements Store with SqliteStore. Re-exports core types so the public API is unchanged. | native only |

### Store trait

Core defines a single Store trait that all storage backends implement.
Operations include: upsert document, replace chunks (with optional vectors),
upsert embedding, get document (full and metadata-only), keyword search, and
vector search. The hybrid search algorithm in core operates only on the Store
trait -- no SQL, sqlx, or backend-specific code in core.

Phase 0 delivers two implementations:
- **In-memory store** (in core) -- brute-force cosine vector search, suitable
  for WASM and testing.
- **SqliteStore** (in the app) -- existing FTS5 + `chunk_vectors` behind the
  same trait, preserving current behavior.

Future backends (Qdrant, Weaviate, etc.) implement the same trait in the app
or in optional crates without changing core.

### Contract preservation

The public API of `context-harness` (modules, re-exports, function signatures)
remains unchanged. `search_documents`, `get_document`, `Config`, `Connector`,
`Tool`, `ToolContext`, `SourceItem`, and all other public symbols are
re-exported at the same paths. CLI and MCP behavior are unchanged. Existing
consumers require no code changes.

### What moves to core

- Models: `Document`, `Chunk`, `SourceItem`, `SearchResultItem`,
  `ScoreExplanation`, `ChunkCandidate`
- Chunking: `chunk_text` (paragraph-boundary chunker)
- Embedding: `EmbeddingProvider` trait + tract implementation
- Search: hybrid scoring algorithm (normalize, merge, aggregate, sort)
- Vector utilities: `cosine_similarity` (and optionally `vec_to_blob`,
  `blob_to_vec`)
- Store trait + in-memory store

### What stays in the app

- Config, db, migrate, ingest, connectors (fs, git, s3, script)
- Lua runtime, tool scripts, agent scripts
- Server, MCP bridge
- Traits (Connector, Tool, ToolContext, registries)
- Fastembed, OpenAI, Ollama embedding providers and dispatch
- SqliteStore
- CLI (`main.rs`)

### Core constraints

Core does not depend on tokio, sqlx, `std::fs`, reqwest, axum, or other
native-only crates. It compiles to `wasm32-unknown-unknown`.

## Alternatives Considered

**Keep a single crate with `#[cfg(target_arch = "wasm32")]` gating.**
Avoids the workspace complexity but litters the codebase with conditional
compilation. Every module that touches SQLite, tokio, or filesystem would
need `#[cfg]` blocks. The boundary between "WASM-safe" and "native-only"
code becomes implicit and fragile -- a single unguarded `use sqlx::*` in
core logic breaks the WASM build. A clean crate boundary makes the
constraint explicit and compiler-enforced.

**Extract only the search algorithm; keep everything else in one crate.**
Smaller refactor but does not address the library-publishing or store-
abstraction goals. Consumers still pull in the full dependency tree, and
storage is still hardcoded to SQLite. Would need a second refactor later
when WASM or Qdrant support is added.

**Extract everything into many small crates (models, chunk, search,
embedding, store, connectors, server, etc.).** Maximum separation but
high coordination cost. Each cross-crate change requires updating multiple
`Cargo.toml` files and ensuring version compatibility. For a project of
this size, two crates (core + app) provide the right balance -- clear
boundary with minimal overhead.

**Use dynamic dispatch or plugin loading for stores instead of a trait.**
Dynamic library plugins (`.so`/`.dylib`) or WASM plugins for storage
backends. This introduces ABI instability, unsafe FFI, and distribution
complexity (see ADR-0007 for similar reasoning). A Rust trait with
compile-time dispatch is simpler, safer, and sufficient.

**Defer and only split when WASM is actually implemented.** Delays the
benefit for library publishing and Tauri integration. The Store trait is
needed regardless of WASM -- it enables Qdrant/Weaviate backends and
cleaner testing. Splitting now means the WASM crate can be added later
with zero refactoring of core.

## Consequences

- **Library consumers** can depend on `context-harness` (full stack) or
  `context-harness-core` (lightweight: models, chunking, search, store
  trait, tract embedding). A Tauri app can use either, depending on
  whether it needs connectors and the MCP server.

- **Storage is pluggable.** SqliteStore is the default; alternative backends
  (Qdrant, Weaviate, in-memory) implement the same Store trait. Core's
  search algorithm works identically regardless of the backend.

- **WASM is unblocked.** A future `context-harness-wasm` crate depends only
  on core and uses the in-memory store and tract embedding. No refactoring
  of core required at that point.

- **Build and CI complexity increase slightly.** The workspace has two
  crates to build and test. `flake.nix`, CI workflows, and path references
  must be updated. This is a one-time cost.

- **No behavioral changes.** CLI, MCP, search results, scoring, and all
  public APIs remain identical. The refactor is invisible to end users.

- **Existing ADRs remain valid.** SQLite (ADR-0002), FTS5 (ADR-0003),
  brute-force vector search (ADR-0004), hybrid scoring (ADR-0005),
  chunking (ADR-0006), and all other decisions are preserved -- the code
  moves to different crates but the algorithms and contracts are unchanged.

- **One new abstraction.** The Store trait is the only new design element.
  It adds a level of indirection between the search algorithm and storage,
  which is a well-understood tradeoff (flexibility vs. directness). For the
  SQLite path, performance impact is negligible since the trait methods map
  directly to the existing SQL queries.

## Related

- **Spec:** [WORKSPACE_REFACTOR_SPEC.md](../WORKSPACE_REFACTOR_SPEC.md) --
  authoritative specification for the refactor (workspace layout, Store
  trait operations, migration steps, acceptance criteria).
- **ADR-0002:** SQLite remains the default storage backend via SqliteStore.
- **ADR-0004:** Brute-force vector search moves into SqliteStore (and the
  in-memory store in core). The BLOB format and cosine similarity are
  unchanged.
- **ADR-0007:** The trait-based extension system (Connector, Tool, Agent)
  is unaffected; those traits stay in the app.
- **ADR-0011:** EmbeddingProvider trait and tract move to core; fastembed
  and remote providers stay in the app.
- **ADR-0016:** Nix flake and CI workflows must be updated for the new
  workspace paths.
