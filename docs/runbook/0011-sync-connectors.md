# RUNBOOK-0011: Sync Connectors

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook covers syncing data from configured connectors into the Context Harness database. Use it after workspace init, when adding new connectors, or to refresh content after source changes.

## Prerequisites

- Workspace initialized (see [RUNBOOK-0010](0010-workspace-init.md))
- At least one connector configured in `config/ctx.toml`
- For Git connectors: network access and valid repo URL
- For S3 connectors: `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` set

## Steps

1. Sync all connectors (incremental by default).

   ```bash
   ctx --config config/ctx.toml sync all
   ```

   Expected output (or similar):

   ```
   filesystem:docs  documents=42  chunks=156  embeddings_written=0  embeddings_pending=156
   filesystem:root  documents=3   chunks=12   embeddings_written=0  embeddings_pending=12
   ...
   ```

2. Sync a specific connector type (all instances of that type).

   ```bash
   ctx --config config/ctx.toml sync filesystem
   ```

   Expected: Only filesystem connectors run; output shows per-connector stats.

3. Sync a specific named connector.

   ```bash
   ctx --config config/ctx.toml sync filesystem:docs
   ```

   Expected: Only the `docs` filesystem connector runs.

4. Perform a full re-sync (ignore checkpoints; re-scan everything).

   ```bash
   ctx --config config/ctx.toml sync all --full
   ```

   Expected: All documents re-ingested; useful after changing `include_globs` or `exclude_globs`.

5. Monitor progress. Sync prints per-connector summaries. For large syncs, output may stream as each connector completes.

6. Verify sync results with `ctx sources`.

   ```bash
   ctx --config config/ctx.toml sources
   ```

   Expected output (or similar):

   ```
   filesystem:docs  OK
   filesystem:root  OK
   git:platform     OK
   ```

## Verification

- `ctx sources` shows `OK` for synced connectors.
- `ctx stats` shows non-zero document and chunk counts for synced sources.
- `ctx search "some term"` returns results from synced content (keyword mode works without embeddings).

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `sync all` reports "no connectors" | No `[connectors.*]` sections in config | Add at least one connector (e.g. `[connectors.filesystem.docs]`) and re-run |
| Scan fails with "path not found" | `root` points to missing directory | Verify `root` path is correct and exists; use absolute path if needed |
| Git sync fails with auth error | No credentials for private repo | Configure SSH key or credential helper; for public repos, ensure URL is correct |
| S3 sync fails | Missing or invalid AWS credentials | Set `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`; for MinIO, set `endpoint_url` in connector config |
| `embeddings_pending` is high | Embedding provider disabled or not run | Set `[embedding] provider` in config and run `ctx embed pending` (see [RUNBOOK-0012](0012-manage-embeddings.md)) |
| Sync is slow | Large directory or many files | Use `exclude_globs` to skip irrelevant paths; consider `--limit` for initial testing |

## Related Runbooks

- [RUNBOOK-0010](0010-workspace-init.md) — Initialize a Workspace
- [RUNBOOK-0012](0012-manage-embeddings.md) — Manage Embeddings
