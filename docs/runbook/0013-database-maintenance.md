# RUNBOOK-0013: Database Maintenance

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook covers routine database maintenance for Context Harness: backup, restore, vacuum, WAL checks, stats, migrations, and reset. Use it when preparing for upgrades, recovering from corruption, or reclaiming disk space.

## Prerequisites

- `ctx` CLI binary installed
- No active `ctx serve` or other processes holding the database open (for backup/restore/vacuum)
- Write access to the workspace `data/` directory

## Steps

1. Backup the database. Copy the SQLite file (and optionally WAL files) to a safe location.

   ```bash
   cp ./data/ctx.sqlite ./data/ctx.sqlite.backup.$(date +%Y%m%d)
   ```

   For a consistent backup while the app may be running, also copy WAL files:

   ```bash
   cp ./data/ctx.sqlite ./backups/ctx.sqlite.$(date +%Y%m%d-%H%M%S)
   cp ./data/ctx.sqlite-wal ./backups/ctx.sqlite-wal.$(date +%Y%m%d-%H%M%S) 2>/dev/null || true
   cp ./data/ctx.sqlite-shm ./backups/ctx.sqlite-shm.$(date +%Y%m%d-%H%M%S) 2>/dev/null || true
   ```

   Expected: Backup file(s) exist in target directory.

2. Restore from backup. Stop any processes using the database, then replace the live file.

   ```bash
   # Ensure nothing is using the DB
   cp ./backups/ctx.sqlite.20260228-120000 ./data/ctx.sqlite
   rm -f ./data/ctx.sqlite-wal ./data/ctx.sqlite-shm
   ```

   Expected: Database restored; run `ctx init` if schema may have changed.

3. Vacuum to reclaim space. Reduces file size after large deletes or rebuilds.

   ```bash
   sqlite3 ./data/ctx.sqlite "VACUUM;"
   ```

   Expected: Command completes; database file may shrink.

4. Check WAL mode. Context Harness uses WAL by default.

   ```bash
   sqlite3 ./data/ctx.sqlite "PRAGMA journal_mode;"
   ```

   Expected output: `wal`

5. View database stats with the CLI.

   ```bash
   ctx --config config/ctx.toml stats
   ```

   Expected output (or similar):

   ```
   documents  123
   chunks     456
   embeddings 456
   by source:
     filesystem:docs  42 docs, 156 chunks
     filesystem:root  3 docs, 12 chunks
   ```

6. Migrate between versions. `ctx init` runs migrations automatically when the database exists.

   ```bash
   ctx --config config/ctx.toml init
   ```

   Expected: "Database initialized" or equivalent; schema updated if migrations ran.

7. Reset database (delete and re-initialize). Use when starting fresh.

   ```bash
   rm -f ./data/ctx.sqlite ./data/ctx.sqlite-wal ./data/ctx.sqlite-shm
   ctx --config config/ctx.toml init
   ```

   Expected: Empty database; re-run sync and embed to repopulate.

## Verification

- After backup: backup file exists and `sqlite3 <backup> "SELECT count(*) FROM documents;"` runs.
- After restore: `ctx stats` runs and shows expected counts.
- After vacuum: `ls -la ./data/ctx.sqlite` shows reduced size (if space was reclaimable).
- After reset: `ctx stats` shows zeros; sync and embed repopulate as expected.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `cp` fails: "text file busy" or "resource busy" | Process holds database open | Stop `ctx serve`, Tauri app, or other ctx processes; retry |
| After restore, `ctx init` fails | Corrupt backup or wrong file | Restore from a known-good backup; verify backup with `sqlite3 <file> "PRAGMA integrity_check;"` |
| VACUUM fails: "database is locked" | WAL checkpoint pending or reader active | Stop all processes; run `sqlite3 ./data/ctx.sqlite "PRAGMA wal_checkpoint(TRUNCATE);"` then VACUUM |
| `journal_mode` returns `delete` | WAL was disabled | WAL is default; if changed, run `PRAGMA journal_mode=WAL;` (requires exclusive access) |
| Migrations fail on `ctx init` | Schema conflict or corrupt DB | Backup first; try `ctx init` again; if persistent, consider reset and full re-sync |

## Related Runbooks

- [RUNBOOK-0010](0010-workspace-init.md) — Initialize a Workspace
- [RUNBOOK-0011](0011-sync-connectors.md) — Sync Connectors
- [RUNBOOK-0012](0012-manage-embeddings.md) — Manage Embeddings
