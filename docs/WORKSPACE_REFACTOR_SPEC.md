# Workspace Refactor Spec

**Status:** Authoritative  
**Scope:** Cargo workspace split and Store abstraction for Context Harness.  
**Policy:** Implementation SHALL conform to this spec. The spec is the single source of truth for workspace layout, trait contracts, and public API preservation.

**Summary.** The repository SHALL be refactored into a Cargo workspace with two crates: `context-harness-core` (WASM-safe: models, chunking, Store trait, in-memory store, embedding trait + tract, search algorithm) and `context-harness` (CLI, server, connectors, SQLite, config; implements Store as SqliteStore; re-exports core so the public API is unchanged). The same contracts and behavior SHALL be preserved; only structure and the introduction of the Store abstraction change.

---

## 1. Definitions

- **Core** -- The crate `context-harness-core`. Shared logic that SHALL compile to both native and `wasm32-unknown-unknown`. It SHALL NOT depend on tokio, sqlx, std::fs, or other native-only crates.
- **App** -- The crate `context-harness`. The main library and CLI. Depends on core and SHALL provide the same public API as before the refactor.
- **Store** -- The abstraction in core that backs document/chunk/vector storage and search. All storage backends (SQLite, in-memory, Qdrant, Weaviate, etc.) SHALL implement the same Store trait defined in core.
- **Contract** -- The public Rust API of `context-harness` (modules, `pub use`, function signatures, and types) as consumed by library users. The contract SHALL remain unchanged after the refactor.
- **Phase 0** -- This refactor. Workspace split + Store trait + SqliteStore + in-memory store. No WASM crate, no concrete Qdrant/Weaviate implementations.

---

## 2. Goals and non-goals

**Goals:**

- Split the repository into a Cargo workspace with at least two crates: `context-harness-core` and `context-harness`.
- Preserve the public API of `context-harness` exactly (same modules, re-exports, and function signatures). Existing consumers SHALL not require code changes.
- Preserve CLI and MCP HTTP behavior. No change to CLI subcommands, flags, or MCP endpoints/request-response shapes.
- Introduce a Store trait in core so that storage backends (SQLite, in-memory, and future Qdrant/Weaviate) are pluggable. Core's search algorithm SHALL operate only on the Store trait and SHALL NOT contain SQL or backend-specific code.
- Enable publishing `context-harness` as a Rust library and using it from a native Tauri app or other binaries without WASM.
- Keep core WASM-safe so that a future `context-harness-wasm` crate MAY depend only on core.

**Non-goals (out of scope for this spec):**

- Implementing the WASM crate or wasm-bindgen.
- Implementing concrete Store backends for Qdrant, Weaviate, or other vector DBs (the trait is designed so they MAY be added later in the app or in optional crates).
- Changing CLI flags, MCP HTTP contract, or config file format.
- Splitting fastembed into a separate crate (optional refinement; not required for Phase 0).
- Adding a server-only binary (document when/if needed).
- Changing the default embedding feature flags. The current `default = ["local-embeddings-fastembed"]` and `local-embeddings-tract` features SHALL be preserved as-is in the app crate. A decision on simplifying build flags is deferred.

---

## 3. Workspace layout

### 3.1 Repository structure

- The repository root SHALL contain a `Cargo.toml` that defines a workspace with `members = ["crates/context-harness-core", "crates/context-harness"]`.
- The current single-crate package SHALL be moved under `crates/context-harness/` (including its `Cargo.toml`, `src/`, `config/`, and any other assets). The root SHALL NOT contain a `src/` directory or a single top-level package after the refactor.
- Building and testing from the repository root (`cargo build`, `cargo test`) SHALL succeed and SHALL exercise both crates as needed.
- Any file-system paths referenced from code, tests, CI, or configuration (e.g. `flake.nix`, `.github/workflows/`, `README.md`, `docs/`, integration tests that reference `config/ctx.toml` or data files) SHALL be updated to reflect the new `crates/context-harness/` layout. The Nix flake SHALL be updated so that `nix build` and `nix develop` continue to work.

### 3.2 Crate roles

| Crate | Purpose | Targets | Dependencies |
|-------|---------|---------|--------------|
| **context-harness-core** | Shared models, chunking, Store trait, in-memory store, embedding trait, tract embedding, search algorithm, vector utilities (cosine, blob encode/decode) | native, wasm32 | serde, uuid, sha2, anyhow, chrono; optional: tract-onnx, tokenizers, ndarray (for tract). No tokio, sqlx, std::fs, reqwest, axum, etc. |
| **context-harness** | CLI, server, connectors, config, db, migrate, ingest, traits, tools, agents, MCP | native only | context-harness-core, tokio, sqlx, clap, axum, etc. Implements Store with SqliteStore. Re-exports core types/functions so the public API is unchanged. |

### 3.3 WASM

- Phase 0 SHALL NOT add a `context-harness-wasm` crate. The workspace SHALL have exactly two members for Phase 0.
- Core SHALL be written so that it compiles for `target wasm32-unknown-unknown` (with tract-based embedding and in-memory store only; no sqlx/tokio in core).

---

## 4. Store trait (core)

### 4.1 Responsibility

The Store trait in core is the single abstraction for:

- Storing and retrieving documents and their chunks.
- Storing and retrieving chunk embedding vectors.
- Keyword search (optional): returning a list of chunk-level candidates with raw scores and snippets.
- Vector search: given a query vector, returning a list of chunk-level candidates with similarity scores and snippets.
- Document metadata lookup by id (needed for the final search-result assembly step; see below).

Core's search algorithm SHALL call only the Store trait (and an optional EmbeddingProvider for query embedding when the store does not perform it). Core SHALL NOT contain SQL, sqlx, or any backend-specific code.

### 4.2 Required operations

The Store trait SHALL support at least the following operations. Exact method names and signatures SHALL be defined in the core crate; the following specifies semantics.

1. **Upsert document**  
   Input: a normalized document (e.g. core type equivalent to `Document` / normalized from `SourceItem`).  
   Effect: insert or replace the document by `(source, source_id)` or by id.  
   Output: document id (e.g. UUID string).

2. **Replace chunks for a document**  
   Input: document id, ordered list of chunks (each with id, document_id, chunk_index, text, content_hash).  
   Optional input: per-chunk embedding vectors (same order as chunks).  
   Effect: remove existing chunks for that document; insert the new chunks and, if provided, their vectors.  
   Output: success or error.

3. **Upsert embedding for a chunk**  
   Input: chunk id, document id, embedding vector, model name, dims, content hash.  
   Effect: insert or replace the embedding (and vector blob) for that chunk.  
   Output: success or error.  
   This operation is needed because embedding can happen separately from chunk creation (e.g. `ctx embed pending`). The app's `embed_cmd` SHALL call this Store operation rather than writing SQL directly.

4. **Get document by id**  
   Input: document id.  
   Output: full document metadata, body, and ordered list of chunks (each with index and text), or "not found".  
   The shape SHALL be equivalent to the current `DocumentResponse` (see `get.rs` / SCHEMAS.md) so that CLI and MCP behavior are unchanged.

5. **Get document metadata by id**  
   Input: document id.  
   Output: document-level metadata only (id, title, source, source_id, updated_at, source_url), or "not found".  
   This is needed because the search algorithm, after scoring and aggregation, must fetch metadata for each matching document to assemble the final `SearchResultItem`. Today this is a `SELECT ... FROM documents WHERE id = ?`. In core, this SHALL be a Store call so that no SQL is in core.

6. **Keyword search**  
   Input: query string, candidate limit (e.g. `candidate_k_keyword`), optional filters (source, since date).  
   Output: list of chunk candidates, each with: chunk_id, document_id, raw_score (backend-specific; e.g. BM25 rank negated), snippet.  
   Backends that do not support full-text search MAY return an empty list. Core's algorithm SHALL treat an empty keyword result set as "no keyword candidates" and SHALL NOT fail.

7. **Vector search**  
   Input: query vector (e.g. `Vec<f32>`), candidate limit (e.g. `candidate_k_vector`), optional filters (source, since date).  
   Output: list of chunk candidates, each with: chunk_id, document_id, raw_score (e.g. cosine similarity), snippet.  
   Sorted by raw_score descending; length at most the given limit.

### 4.3 Async vs sync

The current search, get, and ingest functions are `async` (tokio-based). Core SHALL NOT depend on tokio. The Store trait SHALL therefore be **synchronous** (plain `fn`, not `async fn`), OR SHALL use a runtime-agnostic async mechanism (e.g. `async-trait` with no tokio dependency, allowing the caller to drive futures with whatever runtime it has). The recommended approach is:

- **Option A (sync trait):** Store methods are plain `fn`. The in-memory store is naturally sync. SqliteStore wraps `block_on` or uses synchronous SQLite calls internally. Simple and WASM-safe.
- **Option B (async trait, no tokio):** Store methods return futures (via `async-trait`). Core does not depend on tokio; the future is driven by the caller (tokio in the app, a WASM executor in WASM). SqliteStore implementation remains async naturally.

The implementation SHALL pick one approach and document it. Either way, core SHALL NOT pull in tokio.

### 4.4 Candidate type

Core SHALL define a single chunk-candidate type (e.g. `ChunkCandidate`) used for both keyword and vector search results, with at least: `chunk_id`, `document_id`, `raw_score`, `snippet`. The search algorithm SHALL use this type for normalization, merge, and hybrid scoring. The algorithm SHALL NOT depend on how the store produced the candidates.

### 4.5 Implementations required in Phase 0

- **In-memory store** -- Implemented in core. Documents, chunks, and vectors in Rust structs; vector search via brute-force cosine; keyword search MAY be empty or a simple in-memory full-text scan. Used by a future WASM crate.
- **SqliteStore** -- Implemented in the app. SHALL use existing sqlx/FTS5/`chunk_vectors` and SHALL satisfy the Store trait so that ingest and search use it via the same interface as core's search function.

### 4.6 Future backends

- Additional backends (e.g. Qdrant, Weaviate) MAY implement the same Store trait in the app or in optional crates (e.g. `context-harness-store-qdrant`). They are NOT part of Phase 0.
- A composite Store that delegates document/chunk operations to one backend and vector operations to another MAY be added later; core SHALL continue to see a single Store.

---

## 5. Search algorithm (core)

### 5.1 Algorithm location and interface

- The hybrid search algorithm (keyword + vector candidates -> min-max normalization -> weighted merge -> document-level aggregation -> sort and limit) SHALL live in core.
- It SHALL take: a Store (trait), an optional way to obtain a query embedding (trait or callback), query string, mode (keyword | semantic | hybrid), filters (source, since), limit, explain flag, and retrieval tuning parameters (hybrid_alpha, candidate_k_keyword, candidate_k_vector).
- It SHALL return a list of search results in the same shape as today's `SearchResultItem` (id, score, title, source, source_id, updated_at, snippet, source_url, optional explain). Scores SHALL be in [0.0, 1.0]; ordering and aggregation rules SHALL match the current behavior (see docs/HYBRID_SCORING.md if present).
- The app SHALL call this core function with SqliteStore (and the app's embedder for query embedding when in semantic/hybrid mode) and SHALL expose the result as the existing `search_documents(config, ...)` API.

### 5.2 Document metadata lookup after scoring

Today, `search_documents` scores chunks, aggregates by document, then fetches each matching document's metadata (title, source, source_id, updated_at, source_url) via a separate SQL query. The spec accounts for this: the Store trait includes a **get document metadata by id** operation (see SS4.2 item 5). Core's search algorithm SHALL call this Store method for each unique document in the scored results to assemble the final `SearchResultItem`. Source and since filters SHALL be applied at this stage (same as today: filter after scoring, before truncation).

### 5.3 Retrieval tuning parameters

Today, `search_documents` reads `config.retrieval.hybrid_alpha`, `config.retrieval.candidate_k_keyword`, `config.retrieval.candidate_k_vector`, and `config.retrieval.final_limit` from the `Config` struct. Core SHALL NOT depend on the full `Config` type. Instead, core's search function SHALL accept these as plain parameters (e.g. a small `SearchParams` struct or individual arguments). The app SHALL extract them from `Config` and pass them to core.

---

## 6. Embedding: computation vs storage

### 6.1 Embedding computation

- **Core** SHALL define the `EmbeddingProvider` trait and SHALL provide the **tract-only** implementation (same semantics as current `local_tract`). Core SHALL NOT depend on fastembed, OpenAI, or Ollama.
- **App** SHALL retain fastembed and remote embedding providers (OpenAI, Ollama). The app SHALL use them to produce vectors when configured; those vectors SHALL be written through the Store (see below). For semantic/hybrid search, the app SHALL obtain the query embedding (via its configured provider or core's tract provider) and SHALL pass it to core's search when invoking the store's vector search or when core needs to embed the query.
- The existing embedding feature flags (`local-embeddings-fastembed`, `local-embeddings-tract`) and the current default SHALL be preserved as-is in the app crate.

### 6.2 Embedding storage

- Where vectors are stored is entirely the responsibility of the **Store** implementation. SqliteStore SHALL write vectors to the existing `chunk_vectors` (and related) tables. Other stores (Qdrant, Weaviate, in-memory) SHALL store vectors in their backend. Core's search SHALL only request "vector search with this query vector" (or delegate query embedding to the app and then request vector search); it SHALL NOT assume SQLite or any specific backend.

### 6.3 Vector utilities

- `cosine_similarity`, `vec_to_blob`, and `blob_to_vec` are currently in `embedding/mod.rs`. These are pure functions with no native dependencies. `cosine_similarity` SHALL move to core (needed by the in-memory store's vector search). `vec_to_blob` and `blob_to_vec` MAY move to core (useful for any store that serializes vectors) or MAY remain in the app (only SqliteStore needs them today). Either way, the app SHALL re-export them from `context_harness::embedding` so the public API is unchanged.

### 6.4 embed_texts dispatch

- Today, `embed_texts` and `embed_query` are free functions in `embedding/mod.rs` that dispatch on `config.provider` (with `#[cfg]` for fastembed/tract). These functions and the `create_provider` factory SHALL remain in the app. Core's `EmbeddingProvider` trait and the tract implementation SHALL be used by the app's dispatch logic (e.g. the app's `"local"` branch calls core's tract path). The app's `embed_texts`/`embed_query` public API SHALL NOT change.

---

## 7. Chunking and models (core)

- The paragraph-boundary chunker (current `chunk_text` / `chunk` module) SHALL live in core. Both the app's ingest and any future WASM "add document" flow SHALL use it.
- The following types SHALL be defined in core and SHALL be used by both app and (when applicable) WASM: `Document`, `Chunk`, `SourceItem` (or equivalent), `SearchResultItem`, `ScoreExplanation`, `ChunkCandidate`, and any other types needed by the search algorithm. Exact names and fields SHALL match the current semantics (ids, timestamps, body, snippet, score, etc.) so that re-exports preserve the contract.
- `extract` (PDF/OOXML text extraction) uses `std::io::Read` and Rust-only crates (`pdf-extract`, `zip`, `quick-xml`). It does not use tokio or sqlx. It MAY be placed in core (if we want extraction available for WASM) or MAY remain in the app. For Phase 0, it SHALL remain in the app unless extraction's dependencies are confirmed WASM-safe. This is a placement decision, not a contract change.

---

## 8. Public API (contract) preservation

### 8.1 Library surface

- The crate name SHALL remain `context-harness`. Consumers SHALL continue to depend on `context-harness` for the full library.
- `context-harness` SHALL re-export all public types and functions that are currently part of its API. If a type or function moves to core, the app SHALL re-export it from the same module path as today (e.g. `pub use context_harness_core::models::Document` under `context_harness::models` so that `context_harness::models::Document` still exists).
- The following SHALL remain available with the same signatures and semantics:
  - `search::search_documents(config, query, mode, source_filter, since, limit, explain)` -> `Result<Vec<SearchResultItem>>`
  - `get::get_document(config, id)` -> `Result<DocumentResponse>`
  - `config::Config`, `config::load_config`, and related config types
  - `traits::Connector`, `traits::Tool`, `traits::ToolContext`, `traits::ConnectorRegistry`, `traits::ToolRegistry`, `traits::SearchOptions`, and built-in tool types (`SearchTool`, `GetTool`, `SourcesTool`)
  - `models::SourceItem` (and any other model types currently public)
  - `embedding::cosine_similarity`, `embedding::vec_to_blob`, `embedding::blob_to_vec`, `embedding::EmbeddingProvider`, `embedding::create_provider`, `embedding::embed_texts`, `embedding::embed_query`
  - `chunk::chunk_text`
  - `agents`, `embedding`, and other modules as currently exposed in `lib.rs`

### 8.2 CLI and MCP

- CLI subcommands, flags, and output format SHALL NOT change. The binary name SHALL remain `ctx` (or as currently configured).
- MCP HTTP endpoints, request/response shapes, and tool schemas (search, get, sources) SHALL NOT change. The app SHALL continue to serve the same routes and payloads.

---

## 9. What lives where (normative)

| In core only | In app only |
|--------------|-------------|
| `models` (Document, Chunk, SearchResultItem, ScoreExplanation, SourceItem, ChunkCandidate) | `config`, config file loading |
| `chunk` (chunk_text) | `db`, `migrate` |
| Store trait + in-memory store implementation | SqliteStore (implements Store) |
| EmbeddingProvider trait + tract implementation | fastembed, OpenAI, Ollama embedding providers; `embed_texts`/`embed_query` dispatch; `create_provider` factory |
| Search algorithm (normalize, merge, hybrid, over Store) | `ingest`, connectors (fs, git, s3, script), `lua_runtime`, `tool_script`, `agents`, `agent_script` |
| `cosine_similarity` (and optionally `vec_to_blob`/`blob_to_vec`) | `server`, `mcp` |
| | `traits` (Connector, Tool, ToolContext, registries, built-in tools) |
| | `get`, `sources` (wrappers that use Store and re-export response types) |
| | `search` (thin wrapper: build store from config, call core search, return `Vec<SearchResultItem>`) |
| | `embed_cmd`, `export`, `stats`, `progress`, `extract`, `registry` |
| | `main` (CLI) |

---

## 10. Migration steps (order of operations)

Implementation SHALL follow this order. Each step SHALL be verified (build and tests) before proceeding.

1. **Create workspace**  
   Add root `Cargo.toml` with `[workspace] members = ["crates/context-harness"]`. Move the existing package into `crates/context-harness/` (move `Cargo.toml`, `src/`, `config/`, and any paths referenced by the package). Update any path references in tests, CI, `flake.nix`, `README.md`, and `docs/`. Run `cargo build` and `cargo test` at the repository root and fix regressions.

2. **Create core crate**  
   Add `crates/context-harness-core/` with a `Cargo.toml` that has no tokio/sqlx/std::fs. Add minimal dependencies (serde, uuid, sha2, anyhow, chrono; optional tract, tokenizers, ndarray). Move `models` and `chunk` into core. Move `cosine_similarity` (and optionally `vec_to_blob`/`blob_to_vec`) into core. Define the Store trait (operations as in SS4) and the ChunkCandidate type. Implement the in-memory store in core. Move the search algorithm into core so it operates only on the Store trait and optional query embedding, accepting retrieval tuning parameters directly (not via Config). Move the EmbeddingProvider trait and the tract-based implementation into core. Ensure core builds for the host target.

3. **Implement SqliteStore in the app**  
   In `context-harness`, implement the core Store trait using the existing sqlx pool, FTS5, and `chunk_vectors`. Map upsert document, replace chunks (with optional vectors), upsert embedding, get document (full and metadata-only), keyword candidates, and vector candidates to the current SQL. Replace the app's direct search logic with a call to core's search function, passing SqliteStore and the app's embedder (or query-embedding callback) for semantic/hybrid mode.

4. **Wire app to core**  
   Ingest, embed_cmd, get, and search SHALL use core types and core's search. The app SHALL keep `search_documents(config, ...)` and `get_document(config, id)` as the public entry points; they SHALL obtain the store from config (e.g. SqliteStore backed by the existing db), extract retrieval tuning params from Config, call core's search or get, and return the same types as today. CLI and MCP SHALL call these same entry points. Run full test suite and fix any regressions.

5. **Re-exports and stability**  
   From `context-harness` lib, re-export every type and function that is currently part of the public API, using the same module paths and names. Verify that no existing consumer-facing symbol is removed or renamed. Optionally add a smoke test that imports the main symbols from `context_harness` and compiles.

6. **Optional: wasm32 build of core**  
   Ensure `cargo build -p context-harness-core --target wasm32-unknown-unknown` (with appropriate features) succeeds. This is not required for Phase 0 to be "done" but SHALL be achievable without adding a WASM crate.

---

## 11. Acceptance criteria

The refactor SHALL be considered complete when all of the following hold.

- **Build:** `cargo build` and `cargo build --release` at the repository root succeed. `cargo build -p context-harness-core` succeeds. No new warnings in CI (per project policy).
- **Tests:** All existing tests pass. No tests are removed or disabled except by separate decision.
- **Contract:** Any crate that depends on `context-harness` and uses only the public API (modules and re-exports listed in SS8) compiles without change. The same function and type names are available at the same paths.
- **CLI:** `ctx sync`, `ctx search`, `ctx get`, `ctx embed`, `ctx serve mcp`, and other subcommands behave as before (same flags, same output shape and semantics).
- **MCP:** The HTTP server responds to the same endpoints with the same request/response shapes. Tool list and tool invocation (search, get, sources) produce the same results for the same inputs.
- **Store:** Core's search algorithm is implemented without SQL or backend-specific code. SqliteStore and the in-memory store both implement the Store trait. Search results (scores, order, filtering) are unchanged for the same data and config.
- **Nix and CI:** `nix build`, `nix develop`, and all CI workflows succeed with the new workspace layout.
- **Docs:** This spec is committed and any references to "workspace refactor" or "Phase 0" in other docs point to this document.

---

## 12. Multiple embedding storage backends (design note)

The Store trait is designed so that additional backends (e.g. Qdrant, Weaviate) can be added without changing core. The following table summarizes the intended roles; implementation of these backends is out of scope for Phase 0.

| Backend | Metadata + chunks | Keyword search | Vector search | Location |
|--------|--------------------|----------------|---------------|----------|
| SQLite | FTS5 + documents/chunks | FTS5 BM25 | sqlite-vec | context-harness (Phase 0) |
| In-memory | Rust structs | Optional / none | Brute-force cosine | core (Phase 0) |
| Qdrant | Payload or separate | Optional / none | Native | App or optional crate (future) |
| Weaviate | Properties or separate | Optional / none | Native | App or optional crate (future) |

- Backends that do not support full-text SHALL return an empty list for keyword search; hybrid mode then behaves as semantic-only for that store.
- Optional crates (e.g. `context-harness-store-qdrant`) SHALL depend on core and implement core's Store trait; they SHALL NOT be part of the default workspace members for Phase 0.

---

## 13. References

- **SPEC_POLICY.md** -- Spec structure and normative language.
- **Context Harness WASM Client Design** (Obsidian) -- Section 10 "Workspace refactoring"; source of the workspace and Store design.
- **Current codebase** -- `src/lib.rs`, `src/traits.rs`, `src/search.rs`, `src/chunk.rs`, `src/embedding/`, `src/db.rs`, `src/ingest.rs`, `src/get.rs`, `src/models.rs` (pre-refactor).
- **HYBRID_SCORING.md** (if present) -- Scoring and aggregation behavior that core's search algorithm SHALL preserve.
