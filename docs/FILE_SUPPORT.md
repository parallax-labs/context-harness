# Multi-Format File Support — Specification

This document is the **authoritative specification** for multi-format document ingestion in Context Harness. Implementation MUST conform to this spec. Tests SHALL assert the behavior described here.

**Status:** Authoritative  
**Created:** 2026-02

---

## 1. Scope

### 1.1 Supported formats (first release)

The system SHALL support extraction of plain text from the following formats when they are supplied by a connector as raw bytes with a recognized content-type:

| Format | Extension(s) | MIME type | Extraction method |
|--------|--------------|-----------|--------------------|
| PDF | `.pdf` | `application/pdf` | `pdf-extract` crate: `extract_text_from_mem(&bytes)` |
| Word | `.docx` | `application/vnd.openxmlformats-officedocument.wordprocessingml.document` | OOXML: ZIP + parse `word/document.xml` with `quick-xml`; extract text from `<w:t>` elements |
| PowerPoint | `.pptx` | `application/vnd.openxmlformats-officedocument.presentationml.presentation` | OOXML: ZIP + parse each `ppt/slides/slideN.xml`; extract text from `<a:t>` elements |
| Excel | `.xlsx` | `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet` | OOXML: ZIP + parse `xl/sharedStrings.xml` and `xl/worksheets/sheet1.xml` (and other sheets); map cell references to shared strings and concatenate cell text in sheet order |

Formats not listed above (e.g. ODT, RTF, HTML) are out of scope for this spec. The system MAY support them in a future revision; this spec does not define their behavior.

### 1.2 Where extraction runs

Extraction SHALL run in a **pipeline-layer** step. Connectors that produce items requiring extraction SHALL supply raw bytes and content-type; the pipeline SHALL run the appropriate extractor and produce a `SourceItem` with `body` set to the extracted UTF-8 text before the existing upsert/chunk/embed flow. Connectors that supply `SourceItem` with `body` already set (plain text) SHALL NOT be passed through extraction; the pipeline SHALL use the existing path.

---

## 2. Connector contract for binary content

### 2.1 SourceItem with optional raw bytes

The pipeline SHALL accept items that carry either:

- **Text path:** `body` is a non-empty or empty string and no raw bytes are supplied. These items SHALL be ingested as today (no extraction).
- **Binary path:** Raw bytes and content-type are supplied; `body` MAY be empty. The pipeline SHALL run extraction when the content-type is one of the supported MIME types in §1.1. After extraction, the item SHALL have `body` set to the extracted text and SHALL proceed to upsert/chunk/embed.

The representation of "raw bytes" is implementation-defined (e.g. an optional field on a type, or a separate type that the pipeline converts to `SourceItem`). The only requirement is that the pipeline SHALL receive bytes + content-type for binary files and SHALL produce a `SourceItem` with `body` = extracted text and `content_type` = original MIME before upsert.

### 2.2 Filesystem connector

The filesystem connector SHALL infer binary extraction from file extension (no separate opt-in). For each file matching include_globs and not matching exclude_globs:

1. If the file extension is one of `.pdf`, `.docx`, `.pptx`, `.xlsx`, the connector SHALL read the file as raw bytes and SHALL set content-type from the extension (see §4). The connector SHALL supply the item to the pipeline as a binary-path item (bytes + content_type; body empty or placeholder).
2. For all other files, the connector SHALL attempt to read the file as UTF-8 text. If successful, the connector SHALL supply a text-path item (body = content, content_type = `text/plain` or a detected MIME). If reading as UTF-8 fails and the extension is in the supported list (`.pdf`, `.docx`, `.pptx`, `.xlsx`), the connector SHALL treat the file as binary and supply bytes + content-type. Otherwise the connector SHALL skip the file (do not emit an item) and SHALL NOT fail the scan.

---

## 3. Content model

- **Document / SourceItem:** The stored document SHALL have `body` = extracted plain text (UTF-8) and `content_type` = the **original** MIME type (e.g. `application/pdf`). No schema change to the `documents` table is required; existing columns SHALL be used.
- **dedup_hash:** The formula SHALL remain H(source || source_id || updated_at || body). Extracted content participates in the same formula; re-sync with unchanged file SHALL not change the document.
- **metadata_json:** The system MAY store format-specific metadata (e.g. page_count, sheet_names) in `metadata_json` with reserved keys. This spec does not require it for the first release.

---

## 4. Configuration

### 4.1 Filesystem connector

The following keys SHALL be supported under `[connectors.filesystem.<name>]`:

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `max_extract_bytes` | integer | `50_000_000` | Files larger than this (bytes) SHALL NOT be extracted; they SHALL be skipped (SHALL NOT be ingested). The connector MAY skip such files before supplying them to the pipeline; when skipped at the connector, they need not be counted in the "extraction skipped" summary. A future revision MAY add a separate "connector skipped (oversize)" count. |

Binary extraction is inferred from file extension: any file with extension `.pdf`, `.docx`, `.pptx`, or `.xlsx` that matches include_globs SHALL be read as raw bytes and passed to the pipeline. No separate opt-in is required.

Content-type for binary files SHALL be derived from extension when not provided by the system: `.pdf` → `application/pdf`, `.docx` → `application/vnd.openxmlformats-officedocument.wordprocessingml.document`, `.pptx` → `application/vnd.openxmlformats-officedocument.presentationml.presentation`, `.xlsx` → `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`.

---

## 5. Extraction behavior

### 5.1 PDF

- The implementation SHALL use the `pdf-extract` crate. Text SHALL be extracted via `pdf_extract::extract_text_from_mem(&bytes)`.
- If extraction returns an error (corrupt, password-protected, or unsupported), the item SHALL be skipped (§6). The implementation SHALL NOT panic.

### 5.2 OOXML (docx, pptx, xlsx)

- The implementation SHALL use the `zip` crate to read the OOXML package and the `quick-xml` crate to parse XML.
- **docx:** SHALL read `word/document.xml` from the ZIP and SHALL extract text from `<w:t>` elements in document order. Namespace handling SHALL allow the default and `w:` prefix as in the OOXML schema.
- **pptx:** SHALL read each `ppt/slides/slideN.xml` in order (N from 1) and SHALL extract text from `<a:t>` elements. Namespace handling SHALL allow the default and `a:` prefix.
- **xlsx:** SHALL read `xl/sharedStrings.xml` for the shared strings array and SHALL read each `xl/worksheets/sheetN.xml` in order; SHALL resolve cell values that reference shared strings and SHALL concatenate cell text in row/column order with a single space between cells. Cells without text SHALL be skipped. Implementation MAY limit the number of sheets or cells per sheet to avoid unbounded memory; any such limit SHALL be documented.
- If ZIP or XML parsing fails (corrupt or unexpected structure), the item SHALL be skipped (§6).

---

## 6. Failure modes

- **Per-file extraction failure:** If extraction for a single file fails (library error, corrupt, password-protected, or unsupported), the implementation SHALL skip that item (SHALL NOT upsert it), SHALL log a warning that includes the source_id (or path), and SHALL continue processing the remaining items. The sync SHALL NOT fail.
- **Skip count:** At the end of sync for a connector, the implementation SHALL report the number of items skipped due to extraction failure (e.g. `extraction skipped: N`). The format SHALL be machine-parseable (same line shape as existing `upserted documents: N`).
- **Connector-level failure:** If the connector cannot read the directory or cannot list files, the connector SHALL return an error and the sync SHALL fail as today. This spec does not change connector-level error behavior.

---

## 7. Incremental sync

- Checkpoint and dedup_hash behavior SHALL remain unchanged. Extracted content SHALL be stored in `body`; `dedup_hash` SHALL be computed from source + source_id + updated_at + body. Re-sync without file changes SHALL not re-run extraction (connector returns same items; checkpoint filters unchanged; no change to stored documents).

---

## 8. Acceptance criteria (testable)

The following SHALL hold when the implementation is complete:

1. **PDF ingest and search:** With a filesystem connector whose `include_globs` includes `**/*.pdf`, after `ctx sync filesystem:<name>` on a directory containing a PDF with known text, `ctx search "<phrase from pdf>"` SHALL return at least one result whose snippet contains that phrase.
2. **Idempotent re-sync:** After a successful sync, running `ctx sync` again without modifying any file SHALL result in the same document count and the same dedup_hash for each document.
3. **Skipped on failure:** If a PDF is corrupt or password-protected, sync SHALL complete successfully; the sync output SHALL include `extraction skipped: 1` (or higher); the remaining items SHALL be ingested.
4. **Content-type stored:** For an ingested PDF, the stored document SHALL have `content_type` equal to `application/pdf`. For docx/pptx/xlsx, `content_type` SHALL match the MIME in §1.1.
5. **Office format:** The same as (1) SHALL hold for at least one of .docx, .pptx, or .xlsx (e.g. a docx with known text is synced and search returns a snippet containing that text).
6. **Max size:** A file larger than `max_extract_bytes` SHALL be skipped (SHALL NOT be ingested). When the connector skips the file before passing it to the pipeline, it need not be counted in the extraction-skipped total; the test SHALL assert that the oversize file is not ingested and that the remaining items are ingested as expected.

---

## 9. Dependencies

- **PDF:** The `pdf-extract` crate (MIT). The implementation SHALL depend on `pdf-extract` and SHALL call `extract_text_from_mem` for PDF bytes.
- **OOXML:** The `zip` and `quick-xml` crates. The implementation SHALL NOT require external binaries. Optional feature flags MAY be used to exclude extraction from minimal builds; if used, the default build SHALL include file support.

---

## 10. Summary

| Requirement | Spec |
|-------------|------|
| Formats | PDF, .docx, .pptx, .xlsx only; extraction method per §1.1 and §5. |
| Where | Pipeline layer; connectors supply bytes + content-type for binary files. |
| Content model | body = extracted text; content_type = original MIME; dedup_hash unchanged. |
| Config | max_extract_bytes (default 50_000_000). Binary extraction inferred from extension. |
| Failures | Skip and log per file; report extraction skipped count; do not fail sync. |
| Tests | §8 acceptance criteria SHALL be asserted by tests. |
