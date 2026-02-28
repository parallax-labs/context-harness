# PRD-0004: Local-First Embeddings

**Status:** Delivered
**Date:** 2026-02-23 (conceived) / 2026-02-27 (documented)
**Author:** pjones

## Problem Statement

Semantic search requires embeddings, and most RAG tools depend on cloud
APIs (OpenAI, Cohere) to generate them. This creates three problems:

1. **Privacy:** Document content is sent to a third party. Teams with
   sensitive data (security docs, incident reports, credentials
   references) cannot use cloud embedding APIs.
2. **Cost and availability:** API calls cost money and require network
   access. Offline use, air-gapped environments, and cost-sensitive
   users are excluded.
3. **Setup friction:** Requiring an API key before semantic search works
   is a barrier to adoption. New users who just want to try hybrid
   search must first create an account, generate a key, and configure it.

Context Harness needs semantic search that works out of the box with
zero API keys, zero network calls, and zero cost -- while still
supporting cloud providers for users who prefer them.

## Target Users

1. **Privacy-conscious users** who cannot or will not send document
   content to cloud APIs.
2. **Offline / air-gapped users** who need semantic search without
   network access.
3. **New users** who want hybrid search to work immediately after
   `cargo install` without configuring an API key.
4. **Cost-sensitive users** who do not want to pay per-embedding for
   personal knowledge bases.

## Goals

1. Provide local embedding generation that works offline with zero
   API keys using bundled or pure-Rust ONNX models.
2. Support multiple local backends: fastembed (bundled ONNX Runtime)
   and tract (pure Rust ONNX, no native dependencies).
3. Preserve support for cloud providers (OpenAI, Ollama) as opt-in
   alternatives.
4. Make embedding failures non-fatal: sync succeeds even if embedding
   fails, and keyword search remains available.
5. Ensure the same embedding model and dimensions are used consistently
   for indexing and querying.

## Non-Goals

- Training or fine-tuning embedding models.
- Supporting dozens of model architectures; a small set of well-tested
  models is sufficient.
- Replacing cloud providers for users who prefer them.

## User Stories

**US-1: Zero-config semantic search.**
A new user installs Context Harness, runs `ctx sync filesystem --full`,
and immediately runs `ctx search "auth flow" --mode hybrid`. Local
embeddings are generated automatically. No API key configuration needed.

**US-2: Air-gapped deployment.**
A security team deploys Context Harness on an air-gapped network. They
configure `provider = "local"` and run the tract-based pure-Rust pipeline.
Embeddings are generated entirely on-device with no network calls.

**US-3: Cloud provider opt-in.**
A user with an OpenAI API key configures `provider = "openai"` and
`model = "text-embedding-3-small"`. Embeddings use OpenAI's API instead
of local models.

**US-4: Graceful degradation.**
A user's embedding configuration is broken (model file missing, API key
expired). Sync completes successfully -- documents and chunks are stored,
but embeddings are skipped. `ctx search --mode keyword` works normally.
`ctx embed pending` can be run later to backfill embeddings.

## Requirements

1. The system SHALL support a `local` embedding provider using fastembed
   (bundled ONNX Runtime) as the default on most platforms.
2. The system SHALL support a `local` embedding provider using tract-onnx
   (pure Rust) as an alternative, selectable via feature flags
   (`local-embeddings-tract`).
3. The system SHALL support `openai` and `ollama` embedding providers
   as opt-in alternatives configured in `ctx.toml`.
4. Local embedding models SHALL be bundled with the binary or downloaded
   on first use, depending on the backend.
5. The system SHALL use the same model and dimensions for both indexing
   (`ctx embed pending`) and query-time embedding.
6. Embedding failures during sync SHALL be non-fatal: the document and
   chunks are stored; embeddings can be generated later with
   `ctx embed pending`.
7. The system SHALL support a `disabled` provider that skips embedding
   entirely, leaving keyword-only search available.
8. Embedding provider selection SHALL be configurable via `[embedding]`
   section in `ctx.toml` with `provider`, `model`, and `dimensions`
   keys.

## Success Criteria

- A fresh install with no configuration produces working hybrid search
  using local embeddings.
- Embedding generation runs at >50 chunks/second on consumer hardware
  (M1 Mac, mid-range x86_64).
- Sync completes successfully even when embedding fails.
- The tract backend compiles and runs on all six supported targets
  including `x86_64-unknown-linux-musl` (static binary).

## Dependencies and Risks

- **Model size:** Embedding models (e.g., all-MiniLM-L6-v2) add ~20-80MB
  to the binary or first-run download. This is acceptable for a developer
  tool but should be documented.
- **fastembed native dependency:** fastembed bundles ONNX Runtime, which
  has native compilation requirements. The tract backend provides a
  fallback with zero native dependencies.
- **Quality parity:** Local models may produce lower-quality embeddings
  than cloud models (e.g., OpenAI text-embedding-3-small). For the
  target use case (project documentation, not web-scale search), the
  quality difference is acceptable.
- **Feature flag complexity:** Two local backends behind feature flags
  adds build complexity. Addressed by Nix flake which manages the
  correct flags per target.

## Related Documents

- **ADRs:** [0011](../adr/0011-local-first-embedding-providers.md),
  [0017](../adr/0017-rustls-over-openssl.md)
- **Specs:** [USAGE.md](../USAGE.md) (`[embedding]` config and
  `ctx embed` command)
- **PRDs:** [PRD-0001](0001-core-context-engine.md) (core engine that
  uses embeddings for semantic search),
  [PRD-0006](0006-workspace-refactor-and-library-publishing.md) (tract
  moves to core for WASM compatibility)
