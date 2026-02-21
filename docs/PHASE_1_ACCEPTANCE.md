# Context Harness — Phase 1 Acceptance Criteria

Phase 1 is considered complete only when **all criteria below are satisfied**.

## Phase 1 Scope

- CLI scaffold
- Config parsing
- SQLite schema + migrations
- Filesystem connector
- Ingestion pipeline
- Keyword search (FTS5 over chunks)
- `get` command
- Deterministic behavior
- No embeddings

---

## 1. CLI Contract

- Binary name MUST be `ctx`
- All commands MUST support `--config <path>`
- Required commands: `init`, `sources`, `sync`, `search`, `get`

---

## 2. Configuration

Required TOML structure must parse:

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
```

- Missing sections → clear error
- Invalid types → clear error

---

## 3. Database Initialization

`ctx init` must:
- Create database file if missing
- Create tables: documents, chunks, checkpoints, chunks_fts
- Enable WAL mode
- Be idempotent

---

## 4. Filesystem Connector

`ctx sync filesystem` must:
- Walk configured root
- Match include/exclude globs
- Produce SourceItems with correct field mapping

---

## 5. Ingestion Pipeline

- Upsert key: `(source, source_id)`
- Re-running sync MUST NOT create duplicates
- Chunking respects `max_tokens`
- Checkpoints updated after successful sync

---

## 6. Keyword Search

- FTS MUST index `chunks.text`
- `ctx search` queries FTS, ranks by BM25
- Scores normalized to [0, 1]
- Deterministic ordering with tie-breakers

---

## 7. Manual Test Checklist

### Test 1: Fresh Project
```
ctx init
ctx sync filesystem
ctx search "some word"
ctx get <id>
```
Expected: No errors, reasonable output

### Test 2: Incremental Update
1. Modify a file
2. Run sync
3. Only modified file updates

### Test 3: Re-run Safety
Run `ctx sync filesystem` twice without changes.
Expected: No duplicates

### Test 4: Invalid Query
```
ctx search ""
```
Expected: Graceful empty result, no panic

---

## Completion Definition

Phase 1 is complete when:
- All CLI commands function
- Ingestion is deterministic
- FTS search is stable
- Checkpoints work
- Re-runs are idempotent
- No panics in normal usage

