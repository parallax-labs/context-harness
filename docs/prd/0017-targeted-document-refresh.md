# PRD-0017: Targeted Document Refresh

**Status:** Draft
**Date:** 2026-10-02
**Author:** Context Harness contributors

## Problem Statement

Context Harness refreshes indexed content by scanning a configured connector and then
filtering its returned items. An application that has just created or updated a known
document cannot refresh only that item through a supported ingestion contract. It
must rescan the connector, bypass the normal pipeline, or tolerate stale retrieval.

Trusted host applications need a bounded refresh API that reuses canonical
extraction, chunking, embedding, vector-index, and storage behavior without advancing
a connector checkpoint past unseen work or changing unrelated documents.

## Target Users

- Applications that publish or receive known changed documents.
- Connector authors with reliable item-level change notifications.
- Operators who need prompt retrieval freshness without a full source scan.
- Library users embedding Context Harness ingestion.

## Goals

1. Refresh explicit enrolled documents without scanning unrelated connector content.
2. Reuse the same normalization, extraction, chunking, embedding, and index contracts
   as ordinary sync.
3. Preserve stable source/document identity and connector-wide checkpoints.
4. Return structured per-item outcomes suitable for retry and application status.
5. Keep unrelated canonical documents and derived indexes unchanged.

## Non-Goals

- Filesystem watching, hook configuration, or application publication.
- Inferring deletions from absence in a targeted request.
- Replacing full connector sync or checkpoint-based discovery.
- Cross-source writes selected by untrusted model arguments.
- Guaranteeing immediate embeddings when an optional provider is unavailable.

## User Stories

- After my application writes one enrolled file, I refresh that document and make it
  searchable without rescanning the directory.
- I update a document and know superseded chunks no longer appear in results.
- If optional embedding fails, I receive an explicit partial outcome that I can retry.
- I refresh one item without advancing the connector checkpoint or touching another
  document.
- As a connector author, I submit trusted changed `SourceItem` values through the
  same ingestion validation used by full sync.

## Requirements

| ID | Requirement |
|---|---|
| T1 | A trusted host refreshes an explicit bounded set of items under an enrolled connector/source scope. |
| T2 | Targeted refresh validates source identity, stable item ID, size, type, timestamp, and configured extraction limits before writes. |
| T3 | Full sync and targeted refresh share canonical upsert, chunk replacement, embedding, and vector-index synchronization behavior. |
| T4 | Targeted refresh does not advance or rewrite connector-wide discovery checkpoints. |
| T5 | Create/update replaces superseded chunks and preserves unrelated documents and indexes. |
| T6 | Results distinguish successful canonical indexing, pending/failed optional embedding or derived-index work, validation failure, and storage failure. |
| T7 | Retry of the same unchanged item is idempotent. |
| T8 | Existing `ctx sync` semantics and connector interfaces remain compatible. |

## Success Criteria

- Create and update fixtures become visible through search/get without a connector
  scan; old chunks disappear after update.
- Unrelated document rows, chunks, embeddings, and query results remain unchanged.
- Connector checkpoints are byte-for-byte unchanged by targeted refresh.
- Oversized, mismatched-source, malformed, and duplicate items fail before partial
  canonical writes.
- Disabled, successful local, and failed embedding modes produce explicit tested
  outcomes and safe retry behavior.
- Full sync regression tests continue to use the shared ingestion path successfully.

## Dependencies and Risks

The current per-item pipeline is nested inside `ingest::run_connectors` after a full
`Connector::scan`. Extracting it incorrectly could diverge full and targeted behavior
or desynchronize SQLite and derived vector indexes. Caller-supplied source labels
cannot be treated as authority. Deletion requires a separate exact-item contract and
is excluded initially.

## Contract Status

The initial contract is resolved by
[ADR-0027](../adr/0027-targeted-refresh-authority-and-atomicity.md) and
[SPEC-0024](../spec/0024-targeted-document-refresh.md). It uses trusted
host-provided `SourceItem` values under an opaque enrolled-source handle, bounded
whole-batch preflight, per-document canonical transactions, independent derived-work
states, and a library-only create/update API. Connector item-ID resolution, CLI
exposure and exact deletion remain deferred follow-on work.

## Related Documents

- [DESIGN-0017](../design/0017-targeted-document-refresh.md): shared ingestion
  service, scope boundary, outcomes, and delivery plan.
- [SPEC-0002](../spec/0002-workspace-refactor.md): workspace/storage foundations.
- [SPEC-0004](../spec/0004-file-support.md): file extraction behavior.
- [SPEC-0005](../spec/0005-usage-contract.md): existing sync/configuration contract.
- [ADR-0027](../adr/0027-targeted-refresh-authority-and-atomicity.md): authority,
  atomicity, and initial exposure decision.
- [SPEC-0024](../spec/0024-targeted-document-refresh.md): authoritative targeted
  refresh behavior.
- [PRD-0016](0016-durable-background-agent-tasks.md): independent background tasks
  whose applications may request refresh after their own writes.
