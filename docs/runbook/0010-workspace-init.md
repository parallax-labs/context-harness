# RUNBOOK-0010: Initialize a Workspace

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook walks through initializing a Context Harness workspace from scratch. Use it when setting up a new project directory for context ingestion, or when the Tauri app has not auto-created the workspace structure.

## Prerequisites

- `ctx` CLI binary installed (see [RUNBOOK-0002](0002-build-cli.md))
- Write access to the target directory

## Steps

1. Create the directory structure for config and data.

   ```bash
   mkdir -p config data
   ```

   Expected: `config/` and `data/` directories exist.

2. Create `config/ctx.toml` with required sections and at least one connector. Example with a filesystem connector:

   ```bash
   cat > config/ctx.toml << 'EOF'
   [db]
   path = "./data/ctx.sqlite"

   [chunking]
   max_tokens = 700
   overlap_tokens = 80

   [embedding]
   provider = "local"
   model = "all-minilm-l6-v2"

   [retrieval]
   final_limit = 12
   hybrid_alpha = 0.6
   candidate_k_keyword = 80
   candidate_k_vector = 80
   group_by = "document"
   doc_agg = "max"
   max_chunks_per_doc = 3

   [server]
   bind = "127.0.0.1:7331"

   [connectors.filesystem.docs]
   root = "./docs"
   include_globs = ["**/*.md"]
   exclude_globs = []
   follow_symlinks = false
   EOF
   ```

   Expected: `config/ctx.toml` exists with valid TOML.

3. Initialize the database (creates SQLite file and runs migrations).

   ```bash
   ctx --config config/ctx.toml init
   ```

   Expected output (or similar):

   ```
   Database initialized at ./data/ctx.sqlite
   ```

4. Verify the database was created.

   ```bash
   ls -la data/ctx.sqlite
   ```

   Expected: File exists; may show `ctx.sqlite`, `ctx.sqlite-wal`, `ctx.sqlite-shm` (WAL mode files).

5. (Optional) Configure additional connectors. Edit `config/ctx.toml` and add sections for git, s3, or script connectors. Example for a Git connector:

   ```toml
   [connectors.git.platform]
   url = "https://github.com/acme/platform.git"
   branch = "main"
   root = "docs/"
   include_globs = ["**/*.md"]
   shallow = true
   ```

   After editing, run `ctx sources` to confirm connectors are recognized.

## Verification

- `config/ctx.toml` exists and parses without error.
- `data/ctx.sqlite` exists after `ctx init`.
- `ctx --config config/ctx.toml sources` lists configured connectors with status.
- `ctx --config config/ctx.toml stats` runs without error (may show zeros if no sync yet).

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `ctx: command not found` | CLI not installed or not in PATH | Build with `cargo build -p context-harness` and use `target/debug/ctx` or `target/release/ctx`; or install via release binary |
| `config/ctx.toml: parse error` | Invalid TOML syntax | Check brackets, quotes, and trailing commas; validate with a TOML linter |
| `ctx init` fails with "permission denied" | Cannot write to `data/` | Ensure `data/` exists and is writable; check path in `[db].path` |
| Connector not listed in `ctx sources` | Section name or format wrong | Use `[connectors.<type>.<name>]` (e.g. `[connectors.filesystem.docs]`); ensure `root` and `include_globs` are set for filesystem |

## Related Runbooks

- [RUNBOOK-0011](0011-sync-connectors.md) — Sync Connectors
- [RUNBOOK-0012](0012-manage-embeddings.md) — Manage Embeddings
- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI
