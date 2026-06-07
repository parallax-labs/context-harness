# RUNBOOK-0017: Common Errors Reference

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook is a table-driven reference for common errors encountered when using Context Harness (ctx CLI, MCP server, or native app). Each entry includes Symptom, Cause, Fix, and Prevention. Use it when diagnosing failures during sync, embedding, build, or deployment.

---

## 1. Database is locked

| Field | Details |
|-------|---------|
| **Symptom** | `database is locked` or `SQLITE_BUSY` when running `ctx sync`, `ctx embed`, or during indexing |
| **Cause** | SQLite allows only one writer at a time. Another process (e.g. a second `ctx sync`, the MCP server, or the native app) is holding a write lock. |
| **Fix** | Stop all other ctx processes. Ensure only one `ctx sync` runs at a time. If using the MCP server or app, stop them before running CLI sync. Wait a few seconds and retry. |
| **Prevention** | Do not run multiple `ctx sync` instances against the same database. Use a single sync process; schedule syncs with a lock file or process manager to prevent overlap. |

---

## 2. Read-only file system (os error 30)

| Field | Details |
|-------|---------|
| **Symptom** | `Read-only file system (os error 30)` when ctx tries to access paths in `ctx.toml` |
| **Cause** | Docker-style absolute paths (e.g. `/workspace/docs`) in `ctx.toml` when running natively. The path exists in a container but not on the host. |
| **Fix** | The native app auto-remaps Docker paths to the host. For the CLI, update `ctx.toml` to use host-relative paths (e.g. `./docs` or `$HOME/...`). Or run ctx inside the same container/environment where the paths are valid. |
| **Prevention** | Use relative paths in `ctx.toml` when possible. Document path conventions for Docker vs native in project setup. |

---

## 3. ld: library not found for -lc++

| Field | Details |
|-------|---------|
| **Symptom** | Linker error: `ld: library not found for -lc++` during `cargo build` (especially in Nix shell on macOS) |
| **Cause** | Nix shell or cross-compilation environment lacks the macOS SDK path. The C++ standard library is not found. |
| **Fix** | Set `LIBRARY_PATH` before building: `export LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"` then run `cargo build`. |
| **Prevention** | Add the `LIBRARY_PATH` export to your Nix shell or `.envrc` when developing on macOS. See [RUNBOOK-0001](0001-local-dev-setup.md). |

---

## 4. Failed to retrieve model.onnx / ONNX model failures

| Field | Details |
|-------|---------|
| **Symptom** | `Failed to retrieve model.onnx` or fastembed/ONNX runtime errors during embedding |
| **Cause** | The fastembed cache directory is not writable. Default location may be in a read-only or restricted path (e.g. system dir, CI runner). |
| **Fix** | Set `FASTEMBED_CACHE_DIR` to a writable directory: `export FASTEMBED_CACHE_DIR="$HOME/.cache/fastembed"` (or `$TMPDIR/...`). Ensure the directory exists and is writable. |
| **Prevention** | Set `FASTEMBED_CACHE_DIR` in your environment or `ctx.toml`-driven config. Document the requirement in setup runbooks. |

---

## 5. Port conflicts (MCP server can't bind)

| Field | Details |
|-------|---------|
| **Symptom** | `Address already in use` or `bind: permission denied` when starting the MCP server |
| **Cause** | Another process is using the configured port, or the port requires elevated privileges. |
| **Fix** | Change `[server].bind` in `ctx.toml` to a different port (e.g. `127.0.0.1:7332` instead of `7331`). Update Cursor/MCP client config to use the new port. |
| **Prevention** | Document the default port in runbooks. Use high ports (e.g. 7331+) to avoid conflicts with common services. |

---

## 6. Embedding failures during sync

| Field | Details |
|-------|---------|
| **Symptom** | Sync completes but embeddings are missing; search returns no semantic results. Or `ctx embed pending` fails. |
| **Cause** | Embedding is non-fatal during sync. If the embedding provider fails (API key missing, rate limit, network), sync still succeeds but chunks have no vectors. |
| **Fix** | Run `ctx embed pending` after sync to backfill embeddings. Ensure `OPENAI_API_KEY` (or local embedding config) is set. Fix any provider-specific errors (quota, auth) and re-run. |
| **Prevention** | Set `OPENAI_API_KEY` (or equivalent) before sync when embeddings are required. Consider running `ctx embed pending` as part of a post-sync script. |

---

## 7. Git connector auth (private repos)

| Field | Details |
|-------|---------|
| **Symptom** | `fatal: could not read Username for ...` or `Permission denied (publickey)` when syncing a private Git repo |
| **Cause** | Git needs authentication. SSH key not loaded, or credential helper not configured for HTTPS. |
| **Fix** | For SSH: ensure `ssh-agent` has your key (`ssh-add -l`). For HTTPS: configure a credential helper (`git config --global credential.helper store` or use a helper that supports your auth). |
| **Prevention** | Document Git auth requirements in [RUNBOOK-0011](0011-sync-connectors.md). Use SSH keys for automation; ensure `GIT_SSH_COMMAND` or `~/.ssh/config` is correct if needed. |

---

## 8. S3 connector permissions

| Field | Details |
|-------|---------|
| **Symptom** | `AccessDenied` or `403 Forbidden` when syncing from S3 |
| **Cause** | IAM user/role or credentials lack required S3 permissions. |
| **Fix** | Grant `s3:ListBucket` on the bucket and `s3:GetObject` on the objects. Ensure the bucket policy (if used) allows the principal. Verify credentials with `aws s3 ls s3://your-bucket/`. |
| **Prevention** | Document minimum S3 permissions in connector docs. Use least-privilege IAM policies. |

---

## 9. failed to bundle project error running bundle_dmg.sh

| Field | Details |
|-------|---------|
| **Symptom** | `failed to bundle project` or `error running bundle_dmg.sh` when building the Tauri app for macOS |
| **Cause** | Tauri DMG bundling expects specific targets. Default config may include targets that cause the bundler to fail. |
| **Fix** | In `tauri.conf.json`, set `bundle.targets` to `["app"]` (or the appropriate target) so the DMG bundler uses the correct build output. |
| **Prevention** | Ensure `tauri.conf.json` has `targets: ["app"]` (or equivalent) for macOS DMG builds. See [RUNBOOK-0003](0003-build-tauri-app.md). |

---

## 10. Large repo performance

| Field | Details |
|-------|---------|
| **Symptom** | Sync is very slow or times out on large repositories |
| **Cause** | Full clone and indexing of entire repo history is expensive. |
| **Fix** | Use shallow clones: set `shallow = true` in the Git connector config. Use `root` and `include_globs` / `exclude_globs` to limit indexing to relevant paths (e.g. `docs/`, `src/`). |
| **Prevention** | Configure `shallow = true` and `root` filtering in `ctx.toml` for large repos. Document recommended globs in connector setup. |

---

## Related Runbooks

- [RUNBOOK-0001](0001-local-dev-setup.md) — Local Development Setup (linker, Nix)
- [RUNBOOK-0011](0011-sync-connectors.md) — Sync Connectors
- [RUNBOOK-0016](0016-deploy-docs-site.md) — Deploy Documentation Site (build-docs, Git connector)
