# RUNBOOK-0012: Manage Embeddings

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

Configure an embedding provider, generate embeddings for document chunks, rebuild embeddings when changing models, and verify semantic search quality.

## Prerequisites

- `ctx` binary installed (see [RUNBOOK-0002](0002-build-cli.md))
- A workspace with synced documents (see [RUNBOOK-0011](0011-sync-connectors.md))
- For OpenAI: `OPENAI_API_KEY` environment variable set
- For Ollama: Ollama running locally with an embedding model pulled
- For local: no external dependencies (fastembed bundles the ONNX model)

## Steps

### Configure the embedding provider

1. Open your `ctx.toml` and set the `[embedding]` section. Choose one provider:

   **Local (no API key, runs on-device):**

   ```toml
   [embedding]
   provider = "local"
   ```

   The default model is `all-MiniLM-L6-v2` (384 dimensions). The ONNX model is downloaded automatically on first use.

   **OpenAI:**

   ```toml
   [embedding]
   provider = "openai"
   model = "text-embedding-3-small"
   dims = 1536
   batch_size = 64
   ```

   **Ollama:**

   ```toml
   [embedding]
   provider = "ollama"
   model = "nomic-embed-text"
   dims = 768
   url = "http://localhost:11434"
   ```

   **Disabled (keyword search only):**

   ```toml
   [embedding]
   provider = "disabled"
   ```

### Embed pending chunks

2. Run the embed command to generate embeddings for all unembedded chunks:

   ```bash
   ctx embed pending --config ./config/ctx.toml
   ```

   Expected output:

   ```
   Embedding 1,234 pending chunks...
   Done. 1,234 chunks embedded.
   ```

   This is idempotent -- running it again when no chunks are pending is a no-op.

### Rebuild all embeddings

3. If you change the embedding provider, model, or dimensions, rebuild all embeddings:

   ```bash
   ctx embed rebuild --config ./config/ctx.toml
   ```

   This clears all existing embeddings and re-generates them. It can take significant time for large workspaces.

### Verify with semantic search

4. Test that semantic search returns relevant results:

   ```bash
   ctx search "how does authentication work" --mode semantic --config ./config/ctx.toml
   ```

   Expected: results ranked by cosine similarity, with scores in [0.0, 1.0].

5. Compare with hybrid search:

   ```bash
   ctx search "how does authentication work" --mode hybrid --config ./config/ctx.toml
   ```

   Expected: results that combine keyword (BM25) and semantic (cosine) scores.

## Verification

1. Check embedding stats:

   ```bash
   ctx sources --config ./config/ctx.toml
   ```

   The output shows document count, chunk count, and embedded chunk count. Embedded should match total chunks if all are embedded.

2. Verify a semantic query returns non-zero scores:

   ```bash
   ctx search "test query" --mode semantic --explain --config ./config/ctx.toml
   ```

   The `--explain` flag shows the scoring breakdown. Semantic scores should be non-zero.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| "Failed to retrieve model.onnx" | fastembed cache dir not writable | Set `FASTEMBED_CACHE_DIR` to a writable path (the Tauri app does this automatically) |
| OpenAI rate limit errors | Too many requests | Reduce `batch_size` in config (e.g., 16 or 32) |
| Ollama connection refused | Ollama not running | Start Ollama: `ollama serve` |
| Semantic search returns no results | Embeddings not generated | Run `ctx embed pending` |
| Embeddings exist but search quality is poor | Wrong model dimensions | Verify `dims` in config matches the model's actual output. Rebuild if mismatched. |
| Embedding takes too long | Large workspace | Embedding is CPU/GPU intensive. For local provider, expect ~100-500 chunks/sec on modern hardware. Run overnight for large corpora. |
| "embedding provider is disabled" | Provider set to "disabled" | Change `[embedding].provider` to "local", "openai", or "ollama" |

## Related Runbooks

- [RUNBOOK-0011](0011-sync-connectors.md) -- Sync documents before embedding
- [RUNBOOK-0013](0013-database-maintenance.md) -- Database backup before rebuild
- [RUNBOOK-0017](0017-common-errors.md) -- ONNX model and embedding failure details
