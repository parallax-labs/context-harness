# Sync and Embed Progress — Design & Planning

This document specifies how Context Harness will report **observable progress**
during `ctx sync` (and optionally `ctx embed pending`) so users know what has
been scanned, how much is left, and when search is up to date.

**Status:** Implemented (sync progress on stderr; optional JSON; `--progress` / `--no-progress`). Embed pending progress is optional/future.  
**Created:** 2026-02  
**Depends on:** `ingest.rs` (sync pipeline), `main.rs` (CLI)

---

## 1. Purpose and audience

This spec is for implementers and a future implementation session. It defines
what "progress" means, at what granularity it is reported, how it is exposed
(CLI and optionally machine-readable), and how backward compatibility is
preserved. User-facing documentation (e.g. CLI reference updates) should be
added when implementation is complete.

---

## 2. Current state

When users run `ctx sync` (or sync a specific connector), the command:

1. Resolves connector(s), scans each (e.g. filesystem walk or Git list).
2. Filters items by checkpoint, `--since`, `--until`, `--limit`.
3. For each item: upsert document, replace chunks, optionally embed.
4. Updates checkpoint.
5. Prints a **final summary** only: `sync <label>`, `fetched: N items`,
   `upserted documents: N`, `chunks written: N`, `embeddings written/pending`,
   `checkpoint: <ts>`, `ok`.

There is no **live progress** during the run: no indication of which items
have been processed, how many remain, or when search will be usable. For
large repos or large filesystem trees, users cannot tell if they should wait
or if it is safe to start searching.

Reference: [ingest.rs](src/ingest.rs) `run_connectors`, and the final
`println!` block per connector.

---

## 3. Goal

Users should have **observable progress** during sync (and, where relevant,
during `ctx embed pending`) so they know:

- **What has been scanned/processed** — e.g. which connector or how many
  items are already done (or a meaningful summary).
- **How much is left** — total work (e.g. number of items to process) and how
  many remain (or a percentage), when known.
- **When search is usable** — e.g. a clear indication when sync is done and
  search reflects the current run ("search is up to date after this sync").

Progress should be available in the **CLI** (e.g. `ctx sync`). A future
revision may expose progress via the HTTP/MCP server for UIs or automation
that run sync in the background; this spec focuses on CLI.

---

## 4. Definition of progress

### 4.1 What is reported

- **Phase:** Current phase of the sync (e.g. `discovering`, `ingesting`).
  - **Discovering:** Connector is scanning (e.g. walking filesystem, listing
    Git/S3). Total item count may be unknown until this phase completes.
  - **Ingesting:** Items are being upserted, chunked, and optionally
    embedded. Counts are known (total = filtered item list length).
- **Items processed so far:** Number of items (or chunks, for embed) already
  processed in the current phase.
- **Total (when known):** Total number of items (or chunks) to process in the
  current phase. Not available until discovery completes for sync; always
  available for ingest phase and for `ctx embed pending`.
- **Connector label:** Which connector is being synced (e.g. `filesystem:docs`).

### 4.2 Granularity

- **Per connector:** Progress is reported per connector instance. When
  syncing multiple connectors (e.g. `ctx sync all`), each connector's
  progress is reported separately (e.g. "Syncing filesystem:docs … 1,234 /
  5,000" then "Syncing git:platform … 89 / 89").
- **Per phase:** Within a connector, phases are "discovering" (optional; only
  if we expose it) and "ingesting". Ingest is the main user-visible phase
  (items processed / total).
- **Per item:** Optional: emit a progress update after each item (or every
  N items) to avoid flooding. Spec recommends **per-phase with periodic
  updates** (e.g. every 10 or 100 items, or once per second), not every
  single item, to balance usefulness and noise/performance.

### 4.3 Where progress is emitted

- **CLI:** Progress lines on **stderr** so that stdout remains parseable for
  scripts that consume final summary (e.g. "ok", "fetched: N"). Existing
  final summary (sync label, fetched, upserted, chunks, checkpoint, ok) stays
  on **stdout** as today.
- **Optional flag:** A `--progress` (or `--no-progress`) flag may control
  whether progress is shown: default on for TTY, off when stdout is not a
  TTY, or always on when `--progress` is set. Implementation will decide;
  spec requires that progress be additive and not break existing stdout
  consumers.

---

## 5. Total count handling

- **Filesystem / Git:** Total item count is not known until the connector
  scan completes. Options:
  - **Two-phase reporting:** Emit "Phase 1: discovering …" (no total), then
    "Phase 2: ingesting N items" with progress "1 / N", "2 / N", …. Recommended.
  - **N so far:** During ingest, report "ingested: 1,234" without total until
    done, then final "ingested: 5,000". Less ideal but acceptable if two-phase
    is deferred.
- **Spec recommendation:** Two-phase "discovery then ingest" with phase
  labels. Discovery can report "scanning …" or "discovering …" without a
  total; ingest reports "ingesting … n / total".
- **S3 / Lua:** Same as above if the connector returns a list after a full
  listing; otherwise "N so far" is acceptable.

---

## 6. Output modes

### 6.1 Human-friendly (default)

- Progress on stderr in a form that is readable in a terminal, e.g.:
  - `sync filesystem:docs  ingesting  1,234 / 5,000 items`
  - Or a progress bar: `sync filesystem:docs  [=======>    ] 1,234 / 5,000`
- Final summary on stdout unchanged: `sync filesystem:docs`, `fetched: …`,
  `upserted documents: …`, `chunks written: …`, `checkpoint: …`, `ok`.

### 6.2 Machine-readable (optional)

- When a flag is set (e.g. `--progress=json` or `--json-progress`), emit
  progress as **JSON lines** (one JSON object per line) on stderr, e.g.:
  - `{"event":"progress","connector":"filesystem:docs","phase":"ingesting","n":1234,"total":5000}`
- Enables scripts and UIs to parse progress without scraping human-oriented
  text. Exact schema is defined in implementation; spec only requires that
  machine-readable output be opt-in and not break existing stdout consumers.

### 6.3 Backward compatibility

- **Scripts that parse stdout:** Current behavior must remain. The final
  summary (sync label, fetched, upserted, chunks written, embeddings,
  checkpoint, "ok") stays on stdout in the same format. Progress goes to
  stderr (or only when a flag is set). No change to stdout contract.
- **Scripts that check exit code:** Exit code semantics unchanged (0 = success,
  non-zero = failure).
- **Default behavior:** If progress is enabled by default when stderr is a
  TTY, scripts that redirect only stdout are unaffected. If progress is
  disabled by default unless `--progress` is passed, scripts see no new
  output. Implementation will choose; spec requires that existing "ok" and
  final counts remain parseable from stdout.

---

## 7. Embedding progress

- **`ctx embed pending`:** Runs separately from sync; processes a queue of
  chunks that need embeddings. Progress for this command should follow the
  same principles:
  - Report "chunks embedded / total" (or "n / total") on stderr.
  - Total is known (pending chunk count). Granularity: periodic updates (e.g.
    every 10 or 100 chunks) or once per chunk if fast enough.
- **Inline embedding during sync:** If embeddings are enabled, sync already
  embeds chunks inline. Progress can fold into the same "ingesting" phase
  (e.g. "1,234 / 5,000 items (embeddings: 800 pending)") or be a separate
  sub-phase. Implementation may choose; spec only requires that users have a
  way to see that embedding is in progress when it is slow.

---

## 8. Acceptance criteria (for implementation)

The implementation session is done when:

1. **Incremental progress visible:** When running `ctx sync filesystem:docs`
   on a large directory, the user sees incremental progress (e.g. "ingesting
   n / total items" or a progress bar) on stderr during the run.
2. **Final state clear:** When sync completes, the user sees a clear
   indication that sync is done and search is up to date (existing "ok" and
   summary lines suffice; no new requirement beyond current summary).
3. **Backward compatibility:** A script that parses stdout for "ok" and
   "fetched: N", "upserted documents: N" continues to work without change.
   Progress does not appear on stdout.
4. **Total when known:** During the ingest phase, progress reports both
   current count and total (e.g. "1,234 / 5,000"). During discovery (if
   exposed), total may be omitted.
5. (Optional.) Machine-readable progress (e.g. JSON lines) is available
   behind a flag for scripts/UIs.
6. (Optional.) `ctx embed pending` reports progress (chunks embedded /
   total) on stderr in a similar style.

---

## 9. Summary

| Topic              | Decision |
|--------------------|----------|
| What is reported   | Phase (discovering / ingesting), items processed, total when known, connector label. |
| Granularity        | Per connector; per phase; periodic updates during ingest (e.g. every N items or time). |
| Where              | CLI stderr (stdout reserved for final summary). Optional: HTTP/MCP in future. |
| Total count        | Two-phase: discovery (no total) then ingest (n / total). |
| Human vs machine   | Human-friendly default on stderr; optional machine-readable (JSON lines) behind flag. |
| Backward compat    | Final summary and "ok" remain on stdout; progress additive on stderr. |
| Embed progress     | Same principles for `ctx embed pending`; optional for inline embed during sync. |

Implementation is deferred to a separate session; this spec is the contract
for that work.
