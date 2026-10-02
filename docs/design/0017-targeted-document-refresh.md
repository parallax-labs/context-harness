# DESIGN-0017: Targeted Document Refresh

**Status:** Draft
**Date:** 2026-10-02
**Author:** Context Harness contributors
**Related:** [PRD-0017](../prd/0017-targeted-document-refresh.md), [SPEC-0002](../spec/0002-workspace-refactor.md), [SPEC-0004](../spec/0004-file-support.md), [SPEC-0005](../spec/0005-usage-contract.md)

## Context

`ingest::run_sync` resolves connectors, calls `Connector::scan`, collects every
returned `SourceItem`, applies checkpoint/date/limit filters, and then performs
extraction, canonical upsert, chunk replacement, inline embedding, and checkpoint
advancement inside `run_connectors`.

Applications often already know the exact documents they changed. They need the
per-item half of this pipeline without a full scan and without changing discovery
checkpoints. Direct store calls are insufficient because they can omit extraction,
chunking, embedding, or vector-index consistency.

## Proposal

### Shared ingestion service

Extract the per-item logic into a reusable `DocumentIngestor` or equivalent. Full
sync and targeted refresh both call it.

```text
full sync: Connector::scan -> filters -> shared ingest(items) -> checkpoint update
targeted: enrolled scope + explicit items -> shared ingest(items) -> no checkpoint update
```

The service owns extraction limits, `SourceItem` normalization, canonical document
upsert, deterministic chunking, chunk replacement, configured embedding, derived
vector synchronization, and structured results. Full sync retains discovery,
checkpoint filtering, progress aggregation, and checkpoint advancement.

### Source authority

A trusted host obtains an enrolled source handle by resolving configured connector
identity under the canonical workspace. Targeted items must match that handle's
source label and configured restrictions. The model and ordinary project resources
cannot manufacture handles or choose arbitrary sources.

The initial API accepts host-supplied `SourceItem` values because an application may
already hold immutable published bytes. A later connector capability may resolve an
exact item ID itself. The future spec must define whether both forms ship initially.

Every batch has explicit item-count and total-byte bounds. Each item validates source
identity, stable source ID, URL policy where applicable, timestamps, content type,
raw/body exclusivity, and extraction size before canonical writes.

### Atomicity and outcomes

Process one document transactionally for canonical SQLite document/chunk changes.
Derived embeddings or sidecars may have separate failure boundaries under existing
contracts. Return structured per-item outcomes such as:

```text
indexed
indexed_embedding_pending
unchanged
rejected
failed
```

Names and exact guarantees belong in the spec. The result must distinguish canonical
search freshness from optional semantic/derived freshness. Retry of identical content
uses stable identity and hashes and does not duplicate chunks.

Targeted refresh never updates the connector checkpoint. Full sync advances it only
from items discovered by that scan, as today.

### Update and deletion

Version one supports create/update. An update replaces the exact document's chunks so
superseded text is absent from later search. Absence from a targeted batch says
nothing about deletion.

Exact deletion should be a later explicit operation accepting an enrolled source
handle plus stable source ID, with coordinated canonical and derived cleanup. It must
not be smuggled in as an empty body or missing item.

### Public API and diagnostics

Expose the ingestion service and enrolled-scope resolution as supported library APIs.
Use a structured progress/result sink rather than stdout. A CLI may be added only if
there is a clear operator workflow; application integration is the initial goal.

Diagnostics include validation rejection, extraction failure, storage failure,
embedding unavailable/failed, and derived-index failure without logging raw sensitive
content. Dry-run behavior, if included, performs validation and chunk estimation but
no provider or store mutation.

## Alternatives Considered

- **Run a full connector sync:** correct but unbounded relative to known changes and
  can be expensive for large sources.
- **Filter after `Connector::scan`:** still scans everything and risks checkpoint
  coupling; it does not meet the bounded-refresh goal.
- **Let hosts call `Store` methods directly:** bypasses extraction, chunking,
  embedding, and sidecar contracts.
- **Add item lookup to every connector first:** not all applications need connector
  lookup because they already possess trusted bytes. Keep it an optional later trait.
- **Infer deletion from omission:** unsafe for partial batches. Require explicit exact
  deletion in separate work.

## Implementation Plan

1. Characterize the current per-item ingest behavior and transaction boundaries with
   regression tests.
2. Extract a shared ingestion service without changing `ctx sync` behavior.
3. Add host-issued enrolled source scope and bounded batch validation.
4. Expose targeted create/update with structured per-item outcomes.
5. Test disabled/local/failing embeddings and every configured vector-index mode.
6. Test idempotency, checkpoint preservation, superseded-chunk removal, and unrelated
   document isolation.
7. Update the relevant specs before declaring the public behavior authoritative.

## Acceptance Criteria

- Full sync produces equivalent documents/chunks before and after extraction.
- Targeted create/update performs no connector scan.
- Checkpoints remain unchanged.
- Old chunks and vectors for an updated document are removed/replaced consistently.
- Unrelated documents and query results remain unchanged.
- Mismatched source identity and over-limit batches fail before writes.
- Structured outcomes support safe retry after optional derived-index failure.

## Open Questions

1. `SourceItem` input, connector item-ID resolution, or both in version one?
2. Exact enrolled source-handle type and lifetime?
3. Per-document transaction boundary with optional embeddings/vector sidecars?
4. Exact partial-success outcome taxonomy?
5. Library-only first release or a targeted-refresh CLI?
6. Separate PRD/design for explicit targeted deletion?
