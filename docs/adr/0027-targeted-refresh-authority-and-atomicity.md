# ADR-0027: Targeted Refresh Authority and Atomicity

**Status:** Accepted
**Date:** 2026-10-03

## Context

Trusted applications can know exactly which documents changed, but the existing
ingestion entry point begins with a connector-wide scan. A public targeted path must
reuse canonical ingestion without letting caller-supplied labels create write
authority, advancing discovery checkpoints, or claiming that optional semantic
indexes are current when only keyword-search state committed.

Connectors do not share an exact-item lookup capability. Applications also commonly
already possess the bytes they just published. Requiring connector lookup would add
network and process behavior to a path that can remain deterministic and bounded.

The current ingestion path writes a document and its replacement chunks in separate
operations, then performs embedding and vector-sidecar work. That boundary cannot
provide an atomic public create/update guarantee without a targeted canonical write
operation.

## Decision

The first targeted-refresh API SHALL be a library API for trusted compiled hosts. A
host SHALL resolve an opaque enrolled-source handle from the canonical workspace and
configured connector instance, then submit explicit `SourceItem` values under that
handle. Configuration and model/tool input cannot construct a handle. Connector
item-ID lookup, CLI exposure, deletion and dry-run are deferred.

The API SHALL preflight the complete bounded batch before mutation. It SHALL accept at
most 100 items whose aggregate submitted payload is at most 50,000,000 bytes. The
existing configured per-item extraction limit remains independently authoritative.
Batch-structural or item validation failures produce a structured report and no
writes.

After successful preflight, items SHALL execute independently and in request order.
For one item, the document upsert, complete chunk and FTS replacement, and removal of
obsolete canonical embedding/vector rows SHALL be one SQLite transaction. A failure
of that transaction leaves the prior canonical document searchable. There is no
batch-wide transaction.

Embedding-provider calls and configured derived vector-sidecar updates SHALL occur
outside that canonical transaction. Their failure SHALL NOT roll back canonical
keyword-search freshness. Results SHALL report canonical, embedding and sidecar state
separately so a host can retry incomplete derived work without guessing. Targeted
refresh SHALL neither read nor update connector discovery checkpoints.

The normative behavior is defined by
[SPEC-0024](../spec/0024-targeted-document-refresh.md).

## Alternatives Considered

- **Accept source labels directly:** simple, but turns caller-controlled strings into
  cross-source write authority.
- **Require connector item-ID lookup:** preserves connector ownership of bytes, but
  not every connector supports exact lookup and it needlessly rescans or refetches
  content already held by the host.
- **Make the whole batch atomic:** gives all-or-nothing writes, but holds a large
  transaction and prevents useful per-item retry and progress.
- **Include embedding and sidecars in the canonical transaction:** external provider
  calls and derived stores cannot participate safely in one SQLite transaction.
- **Expose a CLI immediately:** adds an untrusted serialization and authority surface
  before an application workflow demonstrates a need for it.

## Consequences

- Compiled hosts have an explicit supported path while configuration remains data,
  not dynamically loaded code or authority.
- The implementation needs an additive store operation that atomically replaces one
  canonical document and its chunks; the existing full-sync behavior remains
  compatible.
- Keyword results may be current while semantic or accelerated indexes are pending or
  failed. Callers must inspect all result dimensions before claiming full freshness.
- Exact connector lookup, deletion, CLI commands and larger streaming batches require
  later contracts rather than implicit extensions of this one.

## References

- [PRD-0017](../prd/0017-targeted-document-refresh.md)
- [DESIGN-0017](../design/0017-targeted-document-refresh.md)
- [DESIGN-0018](../design/0018-local-agent-application-foundation-plan.md)
- [SPEC-0002](../spec/0002-workspace-refactor.md)
- [SPEC-0004](../spec/0004-file-support.md)
- [SPEC-0024](../spec/0024-targeted-document-refresh.md)
