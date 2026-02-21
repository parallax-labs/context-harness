# Context Harness — Usage Contract

This document defines the public surface area of Context Harness.
Anything described here MUST be supported by the implementation.

---

## Overview

Context Harness is a local-first context indexing framework that:

- Ingests data from external systems (connectors)
- Normalizes into a Document model
- Chunks and embeds content
- Stores in SQLite
- Exposes retrieval via CLI and MCP-compatible server

---

## Configuration

Context Harness MUST accept a `--config` flag pointing to a TOML file.

```bash
ctx --config ./config/ctx.toml <command>
```

### Required Config Fields

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"
```

All fields above MUST exist in config schema.

---

## CLI Commands

### 1. init

Initializes database schema.

```bash
ctx init
```

Required behavior:
- Create SQLite database if missing
- Create required tables
- Create FTS index
- Print success message
- Must be idempotent

---

### 2. sources

Lists available connectors and their status.

```bash
ctx sources
```

Output:

```
filesystem  OK
slack       NOT CONFIGURED
github      NOT CONFIGURED
```

---

### 3. sync

Ingest from connector.

```bash
ctx sync <connector>
```

Required flags:
- `--full`
- `--dry-run`
- `--since <date>`
- `--until <date>`
- `--limit <n>`

Required behavior:
- Use checkpoint unless `--full`
- Upsert documents
- Chunk documents
- Embed if enabled
- Update checkpoint
- Print summary stats

---

### 4. search

Hybrid retrieval (default).

```bash
ctx search "<query>"
```

Required flags:
- `--mode keyword|semantic|hybrid`
- `--source <name>`
- `--since <date>`
- `--limit <n>`

Required behavior:
- Return ranked results
- Show score
- Show snippet
- Show document id

---

### 5. get

Retrieve full document.

```bash
ctx get <id>
```

Required behavior:
- Return metadata
- Return body
- Return chunks
- Handle missing ID with error + nonzero exit

---

## Stability Guarantee

The commands above and their flags are considered the stable public interface.
Implementation must conform to this contract.

