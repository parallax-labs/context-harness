+++
title = "v0.5.1: File Support and Sync Progress"
description = "PDF and Office extraction in the filesystem connector, plus observable progress during sync — no new config required."
date = 2026-02-28

[taxonomies]
tags = ["release"]
+++

Context Harness v0.5.1 ships two improvements that make syncing larger trees and mixed document types smoother: **multi-format file support** (PDF, Word, PowerPoint, Excel) and **sync progress on stderr** so you can see what’s happening during long runs.

---

### Multi-format file support

The filesystem connector now **extracts and indexes PDF, Word (`.docx`), PowerPoint (`.pptx`), and Excel (`.xlsx`)** in addition to plain text. Add those extensions to `include_globs`, run `ctx sync`, and the extracted text is chunked and searchable like everything else. Extraction is inferred from file extension — no extra config flag.

| Format     | Extension | What gets extracted                |
|------------|-----------|------------------------------------|
| PDF        | `.pdf`    | Text via pdf-extract               |
| Word       | `.docx`   | Text from OOXML                    |
| PowerPoint | `.pptx`   | Text from slide XML                |
| Excel      | `.xlsx`   | Cell text (shared strings + order) |

Use optional `max_extract_bytes` (default 50MB) to skip very large files. For the full table, examples, and behavior details, see the [Multi-format file support](https://parallax-labs.github.io/context-harness/blog/multi-format-file-support/) post and the [built-in connectors](https://parallax-labs.github.io/context-harness/docs/connectors/built-in/#supported-file-formats) doc.

---

### Sync progress on stderr

When you run `ctx sync` on a large directory or multiple connectors, you no longer wait with no feedback. Progress is reported on **stderr** so stdout stays parseable for scripts.

- **Discovering** — Shown per connector while the connector is scanning (e.g. walking the filesystem or listing Git).
- **Ingesting** — Once the item list is known, you see `n / total` items, updated every 10 items so the terminal doesn’t flood.

Example:

```
sync filesystem:docs  discovering...
sync filesystem:docs  ingesting  10 / 127 items
sync filesystem:docs  ingesting  20 / 127 items
...
sync filesystem:docs
  fetched: 127 items
  upserted documents: 127
  chunks written: 584
ok
```

The final summary (sync label, fetched, upserted, chunks, checkpoint, `ok`) stays on **stdout** unchanged, so scripts that parse it keep working.

**Options:**

- Default: human-readable progress when stderr is a TTY; no progress when not (e.g. in pipelines).
- `--progress=human` — Force human progress.
- `--progress=json` — One JSON object per line on stderr for scripts or UIs.
- `--no-progress` — Turn off progress (e.g. for clean stdout/stderr in automation).

---

### Upgrading

```bash
# Pre-built binary (replace with your platform: macos-aarch64, macos-x86_64, linux-x86_64, etc.)
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# From source
cargo install --path . --force
```

No breaking config changes. Add `**/*.pdf`, `**/*.docx`, etc. to `include_globs` if you want those formats indexed; use `--no-progress` in scripts if you need to suppress progress output.

---

### What’s next

File support and sync progress are the main highlights for v0.5.1. We’ll keep improving the registry, connectors, and agent workflows in follow-up releases.
