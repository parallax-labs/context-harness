+++
title = "Multi-Format File Support: PDF, Word, PowerPoint, Excel"
description = "The filesystem connector now ingests and indexes PDF and Office documents — no extra config, just add the extensions to include_globs and sync."
date = 2026-02-28

[taxonomies]
tags = ["release", "features"]
+++

Context Harness has always ingested plain text from the filesystem — Markdown, `.txt`, `.rs`, whatever you put in `include_globs`. Now the same connector can **extract and index PDF, Word (`.docx`), PowerPoint (`.pptx`), and Excel (`.xlsx`)**. Add those extensions to your globs, run `ctx sync`, and the extracted text is chunked and searchable like everything else. No separate “enable binary” switch; if the file matches your globs and has a supported extension, it’s in.

---

### What’s supported

| Format   | Extension | What gets extracted                          |
|----------|-----------|----------------------------------------------|
| PDF      | `.pdf`    | Text via `pdf-extract` (searchable PDFs)     |
| Word     | `.docx`   | Text from `word/document.xml` (OOXML)         |
| PowerPoint | `.pptx` | Text from slide XML                          |
| Excel    | `.xlsx`   | Cell text (shared strings + sheet order)     |

Plain text (`.md`, `.txt`, `.rs`, etc.) is unchanged: still read as UTF-8 and indexed directly. For the four binary types above, the connector reads the file as raw bytes, the pipeline runs the right extractor, and the result is stored as document body with the original content-type (e.g. `application/pdf`) so you can filter or display it correctly.

---

### How to use it

Add the extensions you care about to `include_globs`:

```toml
[connectors.filesystem.docs]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt", "**/*.pdf", "**/*.docx", "**/*.pptx", "**/*.xlsx"]
```

Then sync as usual:

```bash
$ ctx sync filesystem:docs
sync filesystem:docs
  fetched: 42 items
  upserted documents: 42
  chunks written: 198
  extraction skipped: 0
ok
```

No `extract_binary = true` or similar — **extraction is inferred from file extension**. If a file matches a glob and has a supported binary extension, it’s read as bytes and passed to the extraction pipeline. Corrupt or password-protected PDFs (and other extraction failures) are skipped and counted in `extraction skipped`; the rest of the sync still succeeds.

---

### Size limit and config

Very large files can be skipped so they don’t blow up memory or CPU. The optional `max_extract_bytes` (default 50MB) caps which files are extracted; anything larger is skipped and included in the extraction-skipped count.

```toml
[connectors.filesystem.docs]
root = "./docs"
include_globs = ["**/*.md", "**/*.pdf"]
max_extract_bytes = 50_000_000   # optional; default 50MB
```

That’s the only extra knob. No per-format flags; the same rule applies to all four binary types.

---

### Under the hood

- **Connector:** For paths matching `include_globs` with extension `.pdf`, `.docx`, `.pptx`, or `.xlsx`, the filesystem connector reads the file with `std::fs::read`, sets content-type from the extension, and emits a “binary path” item (raw bytes + content-type, body empty).
- **Pipeline:** The ingest pipeline accepts items with optional `raw_bytes`. When content-type is one of the supported MIME types, it runs the right extractor (e.g. `pdf-extract` for PDF, OOXML parsing for Office), sets `body` to the extracted UTF-8 text, and then runs the existing upsert/chunk/embed flow. Items that already have a body (plain text) skip extraction.
- **Spec:** We wrote an authoritative spec for this behavior ([FILE_SUPPORT.md](https://github.com/parallax-labs/context-harness/blob/main/docs/FILE_SUPPORT.md)) so implementations and tests stay aligned.

---

### Docs and upgrading

The [built-in connectors](/docs/connectors/built-in/#supported-file-formats) doc has the full table of supported formats and a minimal example. The [configuration reference](/docs/reference/configuration/) and [quick-start](/docs/getting-started/quick-start/) mention the new behavior. No breaking config changes: if you already had `extract_binary = true` in an old config, that key is now ignored (serde skips unknown keys). Just ensure your `include_globs` list the extensions you want, and you’re set.

If you’ve been waiting to point Context Harness at a folder of PDFs and Office docs and search across them, this release is it. Sync, search, and use the same MCP and agent workflows you already have — the new formats slot right in.
