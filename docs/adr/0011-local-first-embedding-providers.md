# ADR-0011: Local-First Embedding Providers

**Status:** Accepted
**Date:** Retroactive

## Context

Semantic search requires vector embeddings of document chunks. The embedding
layer must:

- Work offline without API keys (local-first principle)
- Support cloud providers for users who prefer them (OpenAI, Ollama)
- Handle the six-target build matrix (Linux x86/aarch64/musl, macOS
  x86/aarch64, Windows)
- Detect when chunks change and skip re-embedding unchanged content
- Not block or abort sync when embedding fails (embedding is a value-add,
  not a prerequisite for keyword search)

## Decision

Define an **`EmbeddingProvider` trait** with a factory function
`create_provider()` that returns the appropriate implementation based on
configuration.

### Providers

| Provider | Config value | Implementation | Notes |
|----------|-------------|----------------|-------|
| Disabled | `"disabled"` | Returns error on embed | Keyword search still works |
| Local | `"local"` | `fastembed` (default) or `tract` (fallback) | No API keys, no network |
| OpenAI | `"openai"` | OpenAI embeddings API | Requires `OPENAI_API_KEY` |
| Ollama | `"ollama"` | Local Ollama server | Requires running Ollama instance |

### Platform-Specific Local Backends

- **fastembed** — default on most platforms. Uses ONNX Runtime for fast
  inference. Supports Linux x86_64/aarch64, macOS aarch64 (Apple Silicon).
- **tract** — fallback for platforms where ONNX Runtime is unavailable:
  Linux musl (static builds) and macOS x86_64 (Intel). Pure Rust inference
  engine, slower but fully portable.

The backend selection is compile-time via Cargo feature flags, determined
by the CI build matrix.

### Staleness Detection

Each chunk's text is hashed with SHA-256. The hash is stored in the
`embeddings` table alongside the embedding metadata (model name, dimensions).
During sync or `ctx embed pending`, only chunks whose text hash differs
from the stored hash are re-embedded.

### Sync Integration

Embedding runs **inline during sync** as a non-fatal step. If embedding
fails (provider error, timeout, model loading failure), the chunk is counted
as "pending" and sync continues. Users can retry via `ctx embed pending`.

Remote providers (OpenAI, Ollama) use **exponential backoff** (1s, 2s, 4s, ...)
on transient failures. Embedding is **batched** with a configurable
`batch_size` to optimize throughput and respect API rate limits.

## Alternatives Considered

**Mandatory cloud API (OpenAI only).** Simplest implementation but violates
the local-first principle. Users without API keys or network access cannot
use semantic search. Offline scenarios (air-gapped environments, travel) are
common for the target user base.

**Single local provider (fastembed only).** Would simplify the codebase but
fails on musl and Intel Mac builds where ONNX Runtime is unavailable. The
tract fallback ensures every supported platform has local embedding.

**Embedding at query time only.** Embed the query and compare against
pre-computed chunk embeddings. This is the current approach for the query
side, but the decision here is about pre-computing chunk embeddings during
sync. Computing embeddings at query time for all chunks would be prohibitively
slow (embedding the entire corpus on every search).

**External embedding service (dedicated microservice).** Run a separate
embedding server. Adds operational complexity, a network dependency, and
violates the single-binary design. Ollama support covers the "local server"
use case for users who want that model.

**Store embeddings in a separate file (HDF5, NumPy format).** Would
separate vector storage from the SQLite database. Adds a second file to
manage, complicates backup/restore, and gains nothing since SQLite BLOBs
handle the storage efficiently.

## Consequences

- Users get semantic search out of the box with `provider = "local"` and no
  API keys. This is the primary onboarding path.
- The provider abstraction allows switching between local and cloud providers
  via a single config change, without affecting the rest of the pipeline.
- Platform-specific backends (fastembed vs tract) are transparent to users
  but add complexity to the CI build matrix and Cargo feature configuration.
- Non-fatal inline embedding means sync never fails due to embedding issues.
  The system degrades gracefully to keyword-only search when embeddings are
  unavailable.
- SHA-256 staleness detection avoids redundant API calls and compute during
  incremental sync, which is important for both cost (OpenAI) and time
  (local inference).
- The `EmbeddingProvider` trait is extensible — adding a new provider
  (Cohere, Voyage, etc.) requires implementing `embed()`, `model_name()`,
  and `dims()`.
