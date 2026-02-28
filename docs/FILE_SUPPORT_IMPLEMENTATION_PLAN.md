# Multi-Format File Support — Implementation Plan

This plan implements the behavior specified in [FILE_SUPPORT.md](FILE_SUPPORT.md). The spec is authoritative; each task below maps to spec sections. Implementation SHALL conform to the spec, and tests SHALL assert the acceptance criteria in spec §8.

---

## 1. Authority and order of work

- **Spec:** [docs/FILE_SUPPORT.md](FILE_SUPPORT.md) — all behavior is defined there. No new behavior; only code that satisfies the spec.
- **Order:** Dependencies first (model + config + crates), then extraction module, then filesystem connector, then pipeline integration, then tests. Each phase leaves the crate building and existing tests passing.

---

## 2. Phase 1: Model and config

### 2.1 Add optional raw bytes to SourceItem (spec §2.1)

**File:** [src/models.rs](src/models.rs)

- Add field: `pub raw_bytes: Option<Vec<u8>>` to `SourceItem`. When `Some`, the pipeline SHALL treat the item as binary-path and run extraction; when `None`, use `body` as today (text path).
- Document: "When set, the pipeline runs extraction and sets body from the result before upsert; content_type identifies the format."

**Call sites:** Every constructor of `SourceItem` MUST set `raw_bytes`. Default is `None` for text-path items.

- [src/connector_fs.rs](src/connector_fs.rs) — today sets `body`, `content_type`; will set `raw_bytes: Some(bytes)` for binary files in Phase 3; for text path set `raw_bytes: None`.
- [src/connector_git.rs](src/connector_git.rs) — add `raw_bytes: None` to the single `SourceItem` constructor.
- [src/connector_s3.rs](src/connector_s3.rs) — add `raw_bytes: None` to the `SourceItem` push.
- [src/connector_script.rs](src/connector_script.rs) — add `raw_bytes: None` to the `SourceItem` push (Lua path does not supply binary; script can be extended later if needed).

### 2.2 Filesystem connector config (spec §4.1)

**File:** [src/config.rs](src/config.rs)

- Add to `FilesystemConnectorConfig`:
  - `extract_binary: bool` with `#[serde(default)]` (default `false`).
  - `max_extract_bytes: u64` with `#[serde(default = "default_max_extract_bytes")]`; add `fn default_max_extract_bytes() -> u64 { 50_000_000 }`.
- Update the doc comment example to show optional `extract_binary = true` and `max_extract_bytes = 50_000_000`.

**Deliverable:** `cargo build` succeeds; existing tests pass. No behavior change yet.

---

## 3. Phase 2: Dependencies and extraction module

### 3.1 Cargo dependencies (spec §9)

**File:** [Cargo.toml](Cargo.toml)

- Add:
  - `pdf-extract = "0.10"` (or latest compatible; spec §5.1).
  - `zip = "2"` (or latest compatible).
  - `quick-xml = { version = "0.36", features = ["serialize"] }` (or version used elsewhere if any); `serialize` only if needed for tests.
- Optional: feature `file-support` that enables these; default feature list includes it so default build has file support (spec §9). If not using a feature, add deps unconditionally.

### 3.2 New module: extract (spec §1.1, §5)

**New file:** [src/extract.rs](src/extract.rs)

- **Public API:** One function used by the pipeline, e.g. `pub fn extract_text(bytes: &[u8], content_type: &str) -> Result<String, ExtractError>`. Returns extracted UTF-8 text or an error (spec §6: on error, pipeline skips item).
- **Content-type handling:** If `content_type` is not one of the four MIME types in spec §1.1, return an error (or a dedicated "unsupported" variant) so the pipeline can skip.
- **PDF (spec §5.1):** Call `pdf_extract::extract_text_from_mem(bytes)`. Map library errors to `ExtractError`; do not panic.
- **docx (spec §5.2):** Open bytes as ZIP; find and read `word/document.xml`; parse with `quick-xml` (Reader); extract text from elements with local name `t` in the `w` namespace (handle both `w:t` and default namespace with appropriate name check). Concatenate in document order. On ZIP or XML error, return `ExtractError`.
- **pptx (spec §5.2):** Open bytes as ZIP; list entries under `ppt/slides/` and sort by name (slide1.xml, slide2.xml, ...); read each; parse with quick-xml; extract text from elements with local name `t` in the `a` namespace (`a:t`). Concatenate with space between slides. On error, return `ExtractError`.
- **xlsx (spec §5.2):** Open bytes as ZIP; read `xl/sharedStrings.xml` and parse to build a vector of shared strings; read `xl/workbook.xml` or `[Content_Types].xml` to determine sheet order; for each sheet, read `xl/worksheets/sheetN.xml`, parse `<v>` in cells and resolve via shared strings or inline string; concatenate cell text with space. Spec allows limiting sheets/cells; document the limit (e.g. max 100 sheets, 100k cells) in the module and enforce it.
- **ExtractError:** Enum or opaque error type that implements `std::error::Error` and `Send + Sync`. Used for logging in ingest (spec §6).

**lib.rs:** Add `pub mod extract;` and re-export if desired (e.g. `pub use extract::extract_text` for tests).

**Deliverable:** Unit tests for `extract_text` with minimal fixtures: a tiny PDF with known text, a minimal docx/pptx/xlsx (or use test fixtures in `tests/` or `testdata/`). Extraction returns the expected string; invalid bytes return an error.

---

## 4. Phase 3: Filesystem connector binary path (spec §2.2)

**File:** [src/connector_fs.rs](src/connector_fs.rs)

- **Config:** Pass `FilesystemConnectorConfig` into `scan_filesystem` and into `file_to_source_item` (or have `file_to_source_item` take config and path, and decide text vs binary).
- **Extension set:** Define the set of extensions that trigger binary path: `[".pdf", ".docx", ".pptx", ".xlsx"]`. Map extension to MIME per spec §4.1 (`.pdf` → `application/pdf`, etc.).
- **Per-file logic in `file_to_source_item`:**
  1. If `extract_binary` is true and file extension is in the set: read file with `std::fs::read(path)`. If file size > `max_extract_bytes`, do not emit an item (skip) — but the spec says "SHALL be skipped and SHALL be counted in the skipped summary". So we have two options: (A) skip at connector level and not emit the item (then we cannot count it in "extraction skipped" unless we add a separate mechanism), or (B) emit an item with raw_bytes set and let the pipeline reject it when size > max and count as skipped. Spec §4.1 says "Files larger than this ... SHALL NOT be extracted; they SHALL be skipped and SHALL be counted in the skipped summary." So the count is "extraction skipped". Easiest: connector emits item with raw_bytes; pipeline checks size before calling extract; if over limit, skip and count. So connector always emits binary-path items for matching extensions (and reads bytes); pipeline enforces max_extract_bytes. Alternatively connector checks size and does not emit (then we need another way to report "skipped" for size — e.g. connector could return a separate "skipped" count). Simplest: connector emits all binary-path items; pipeline before extraction checks `item.raw_bytes.as_ref().map(|b| b.len()).unwrap_or(0) > max_extract_bytes` (pipeline needs config or max value). So pipeline needs `max_extract_bytes` in the ingest path. That implies config (or a constant) available in ingest. So: connector emits item with raw_bytes; ingest has access to config; for each item with raw_bytes, if bytes.len() > config.connectors.filesystem.<?> max_extract_bytes we need per-connector config. So we need to pass per-connector config into the ingest loop. Currently ingest only has `Config` and the connector's label. So we could add a way to get "max_extract_bytes for this connector" from config (e.g. by connector label: "filesystem:docs" -> look up connectors.filesystem.docs.max_extract_bytes). So in ingest we have source_label; we can parse "filesystem:name" and look up config.connectors.filesystem.get(name).and_then(|c| c.max_extract_bytes). So: connector does NOT check max_extract_bytes; it emits all binary-path items. Pipeline in ingest checks size against the connector's max_extract_bytes (from config) and if over, skips and counts. That matches spec: "Files larger than this ... SHALL NOT be extracted; they SHALL be skipped and SHALL be counted in the extraction skipped summary."
  2. If extract_binary is true and extension in set: read bytes with `std::fs::read`. Set `content_type` from extension. Set `body` to empty string. Set `raw_bytes = Some(bytes)`.
  3. Else: try `std::fs::read_to_string(path)`. If Ok, set body and content_type "text/plain", raw_bytes None. If Err (e.g. invalid UTF-8) and extract_binary is true and extension in set: fall back to read-as-bytes and set raw_bytes (same as 2). Otherwise: skip file (do not push item) — spec "the connector SHALL skip the file ... and SHALL NOT fail the scan".
- **Signature:** `file_to_source_item` needs the config and the list of binary extensions (or just config). So `file_to_source_item(path, relative_path, source, fs_config: &FilesystemConnectorConfig) -> Result<Option<SourceItem>>` where `None` means "skipped" (invalid UTF-8 and not binary path). Or return `Result<SourceItem>` and use a variant: for "skip" we could return Err with a custom error that the caller treats as skip. Cleaner: return `Result<Option<SourceItem>>`; caller only pushes when `Some`.
- **Call site:** In the walk loop, when we get `Some(item)` push it; when `None` do not push. Do not fail the scan.

**Deliverable:** With `extract_binary = true` and `include_globs` including `**/*.pdf`, the filesystem connector returns items with `raw_bytes = Some(...)` and `content_type = "application/pdf"`. With `extract_binary = false`, behavior unchanged (no raw_bytes).

---

## 5. Phase 4: Pipeline integration (spec §1.2, §6, §7)

**File:** [src/ingest.rs](src/ingest.rs)

- **Before the per-item loop:** For each item, if `item.raw_bytes.is_some()`:
  1. Resolve max_extract_bytes for this connector: from config, using source_label (e.g. parse "filesystem:name" and look up `config.connectors.filesystem.get(name).map(|c| c.max_extract_bytes)`). If the connector is not filesystem, use a default (e.g. 50_000_000) or skip extraction (treat as unsupported).
  2. If `raw_bytes.as_ref().unwrap().len() > max_extract_bytes`: skip this item (do not upsert), increment `extraction_skipped`, log warning with source_id, continue.
  3. Call `extract::extract_text(item.raw_bytes.as_ref().unwrap(), &item.content_type)`. On Err: skip item, increment `extraction_skipped`, log warning with source_id, continue. On Ok(text): set `item.body = text`, set `item.raw_bytes = None` (so downstream sees a normal text item). Then proceed to upsert/chunk/embed.
- **Items with raw_bytes = None:** Proceed as today (upsert, chunk, embed). No change.
- **Count:** Add `let mut extraction_skipped = 0u64`. After the loop, print `println!("  extraction skipped: {}", extraction_skipped);` (same style as "upserted documents: N") so it is machine-parseable (spec §6).
- **Dry-run:** For dry-run, items with raw_bytes are still "items found"; we do not need to run extraction in dry-run. So in dry-run branch, when counting items and estimating chunks, treat raw_bytes items as "body unknown" — e.g. skip them for chunk estimate or use 0. Spec does not mandate dry-run behavior for binary; leave dry-run as-is (items.len() includes binary items; estimated chunks can be 0 for them or omit from sum).

**Deliverable:** Full sync with a connector that has extract_binary = true and PDFs in the directory: PDFs are ingested and searchable; corrupt PDF skips and prints "extraction skipped: 1"; files over max_extract_bytes skip and count toward extraction_skipped.

---

## 6. Phase 5: Tests (spec §8)

Add tests that assert each acceptance criterion. Prefer integration tests that run `ctx sync` against a temp directory and then `ctx search` or query the DB.

### 6.1 Test: PDF ingest and search (spec §8.1)

- Create a temp directory with a PDF that contains known phrase "spec test phrase".
- Configure a filesystem connector (in code or via temp config) with extract_binary = true and include_globs including `**/*.pdf`.
- Run sync (via `run_sync` or CLI).
- Run search for "spec test phrase"; assert at least one result with snippet containing that phrase.

### 6.2 Test: Idempotent re-sync (spec §8.2)

- After the sync above, run sync again. Assert document count unchanged; optionally assert dedup_hash for each document is unchanged (query DB).

### 6.3 Test: Skipped on failure (spec §8.3)

- Add a corrupt or password-protected PDF to the temp dir (or a zero-byte file named .pdf). Sync. Assert sync completes; assert output contains "extraction skipped: 1" (or higher). Assert remaining valid documents are still ingested.

### 6.4 Test: Content-type stored (spec §8.4)

- After ingesting a PDF, get the document from DB (or via get command). Assert content_type == "application/pdf".

### 6.5 Test: Office format (spec §8.5)

- Same as §8.1 but with a minimal .docx (or .pptx or .xlsx) containing known text. Assert search returns a snippet with that text.

### 6.6 Test: Max size (spec §8.6)

- Add a file larger than max_extract_bytes (e.g. create a 60MB file or mock the size check). Assert it is skipped and extraction skipped count includes it. (Implementation may enforce max in connector or pipeline; test the observable behavior.)

### 6.7 Test: extract_binary = false (spec §8.7)

- With extract_binary = false (default), add a PDF to the directory. Sync. Assert the PDF is not ingested (search for its text returns no result, or document count does not include it). Assert no panic and existing text files still work.

**Test location:** [tests/](tests/) integration tests (e.g. `tests/file_support.rs`) or in [src/extract.rs](src/extract.rs) for unit tests and in a new `tests/ingest_file_support.rs` for pipeline tests. Use tempfile for dirs and sample PDFs/docx; minimal binary fixtures can be committed or generated in test.

---

## 7. Summary table

| Phase | Deliverable | Spec ref |
|-------|-------------|----------|
| 1 | SourceItem.raw_bytes; config extract_binary, max_extract_bytes; all constructors set raw_bytes | §2.1, §4.1 |
| 2 | extract.rs (PDF + OOXML); Cargo deps; unit tests for extract | §1.1, §5, §9 |
| 3 | Filesystem connector: binary path, extension→MIME, skip on UTF-8 fail when not binary | §2.2 |
| 4 | Ingest: run extraction for raw_bytes items; max size check; extraction_skipped count and log | §1.2, §6, §7 |
| 5 | Tests for §8.1–§8.7 | §8 |

---

## 8. Files to add or touch

| File | Action |
|------|--------|
| [docs/FILE_SUPPORT.md](FILE_SUPPORT.md) | No change; authority. |
| [docs/FILE_SUPPORT_IMPLEMENTATION_PLAN.md](FILE_SUPPORT_IMPLEMENTATION_PLAN.md) | This plan. |
| [Cargo.toml](Cargo.toml) | Add pdf-extract, zip, quick-xml. |
| [src/lib.rs](src/lib.rs) | Add `pub mod extract`. |
| [src/models.rs](src/models.rs) | Add `raw_bytes: Option<Vec<u8>>`. |
| [src/config.rs](src/config.rs) | Add extract_binary, max_extract_bytes to FilesystemConnectorConfig. |
| [src/extract.rs](src/extract.rs) | New: extract_text, PDF + OOXML. |
| [src/connector_fs.rs](src/connector_fs.rs) | Binary path, config, skip logic. |
| [src/connector_git.rs](src/connector_git.rs) | Add raw_bytes: None. |
| [src/connector_s3.rs](src/connector_s3.rs) | Add raw_bytes: None. |
| [src/connector_script.rs](src/connector_script.rs) | Add raw_bytes: None. |
| [src/ingest.rs](src/ingest.rs) | Extraction step, max size, extraction_skipped. |
| tests/file_support.rs or similar | New: integration tests per §8. |

---

## 9. Risks and mitigations

- **PDF crate panics:** Spec §5.1 says "SHALL NOT panic". Wrap `extract_text_from_mem` in a catch or use a crate that returns Result. If the crate panics on corrupt input, consider `std::panic::catch_unwind` for that call only and convert to ExtractError.
- **OOXML complexity:** xlsx has shared strings and multiple sheets; start with docx (single document.xml), then pptx, then xlsx. Document any sheet/cell limits in extract.rs.
- **Config per connector in ingest:** Ingest currently does not receive per-connector config, only `Config` and connector label. We need to resolve "filesystem:docs" to the docs instance config. Add a helper that, given config and source_label, returns the FilesystemConnectorConfig for that label if any (e.g. parse "filesystem:name", then config.connectors.filesystem.get(name)). Use it for max_extract_bytes.

---

Implementation is complete when all phases are done, `cargo test` passes, and the seven acceptance criteria in spec §8 hold.
