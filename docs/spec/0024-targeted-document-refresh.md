# SPEC-0024: Targeted Document Refresh

**Status:** Authoritative
**Date:** 2026-10-03
**Related:** [PRD-0017](../prd/0017-targeted-document-refresh.md), [DESIGN-0017](../design/0017-targeted-document-refresh.md), [ADR-0027](../adr/0027-targeted-refresh-authority-and-atomicity.md), [SPEC-0002](0002-workspace-refactor.md), [SPEC-0004](0004-file-support.md)

## Scope

This specification defines the first trusted library API for refreshing known
created or updated documents without a connector scan. It does not define connector
item lookup, deletion, a CLI or MCP command, dry-run behavior, background execution,
or model/tool access.

## Definitions

- **Canonical state:** the SQLite document, chunks, FTS rows, and SQLite embedding
  and vector rows owned by Context Harness.
- **Derived sidecar:** a rebuildable vector index outside canonical SQLite.
- **Enrolled source:** an opaque, host-resolved capability binding one canonical
  workspace to one configured connector instance and its effective validation policy.
- **Payload bytes:** the checked sum of `raw_bytes` and the UTF-8 byte lengths of all
  submitted `SourceItem` string fields, including optional fields when present.
- **Preflight:** complete batch validation performed before any store or provider
  operation.

## Authority and construction

The public API SHALL allow a trusted compiled host to resolve an `EnrolledSource`
from the canonical resolved configuration and workspace. Resolution SHALL accept an
exact configured source label, such as `filesystem:docs`, and SHALL fail for an
unknown or ambiguous label.

`EnrolledSource` SHALL expose its source label for inspection but SHALL have no public
unchecked constructor and SHALL NOT implement deserialization. Configuration,
resources, model output, tool arguments and arbitrary strings SHALL NOT dynamically
load code or create enrollment authority.

Enrollment resolution SHALL be side-effect free: it SHALL NOT scan a connector,
access the network, resolve a secret, open a database, invoke a model or provider, or
start a process. The handle SHALL retain the workspace identity, exact source label,
configured extraction limit, and any source validation policy needed by refresh.

The initial API SHALL accept only host-provided `SourceItem` values. It SHALL NOT
accept connector item IDs for resolution and SHALL NOT call `Connector::scan`.
An executor SHALL reject a handle issued for a different canonical workspace.

## Batch contract and preflight

One request SHALL contain between 1 and 100 items inclusive. Aggregate payload bytes
SHALL NOT exceed 50,000,000. Each item SHALL also satisfy the enrolled source's
configured per-item extraction limit. Limits SHALL be checked with overflow-safe
arithmetic.

Preflight SHALL preserve request order and validate every item. Each item SHALL:

- have `source` exactly equal to the enrolled source label;
- have a nonempty `source_id` after rejecting ASCII-whitespace-only values;
- have a nonempty `content_type` after rejecting ASCII-whitespace-only values;
- have `created_at` and `updated_at` values representable by canonical storage;
- contain valid JSON in `metadata_json`, with a JSON object as its top-level value;
- contain either `raw_bytes` with an empty `body`, or no `raw_bytes` and a `body`;
- satisfy the enrolled source's side-effect-free item policy; and
- use a `source_id` that appears only once in the request.

For filesystem and Git enrollments, the item policy SHALL apply the configured root,
include globs and exclude globs to a normalized relative `source_id`; absolute paths
and parent-directory traversal SHALL be rejected. For S3 enrollment, it SHALL require
the configured key prefix and apply configured include/exclude globs. Script
enrollment has no source-specific restriction beyond the common rules because the
script configuration defines no item namespace policy. These checks SHALL operate on
the supplied identity only and SHALL NOT access the filesystem, repository, object
store or script process.

An empty text body is a valid document update and SHALL NOT mean deletion. When
`raw_bytes` is present, extraction SHALL use the existing multi-format extraction
contract in [SPEC-0004](0004-file-support.md). Unsupported or failed extraction is a
validation rejection, not an empty document.

Preflight SHALL NOT read or write the database, invoke an embedding provider, update
a sidecar, scan a connector, or start a process. If any batch or item rule fails, the
API SHALL return an ordered report containing a sanitized rejection code for each
rejected item and `not_attempted` for every other item. It SHALL perform no canonical
or derived writes and no provider calls.

## Execution and canonical transaction

After successful preflight, the API SHALL process items sequentially in request
order. Each item is independent; there is no batch-wide transaction. A storage
failure for one item SHALL be recorded for that item and SHALL NOT prevent later
items from being attempted.

For each item, normalization and deterministic chunking SHALL use the same behavior
as full sync. The following changes SHALL commit in one SQLite transaction:

1. insert or update the exact `(source, source_id)` document;
2. replace all chunks and FTS rows for that document; and
3. remove obsolete SQLite embedding and vector rows for replaced chunks.

If that transaction fails, it SHALL roll back completely and the previously committed
canonical document and chunks SHALL remain visible. It SHALL NOT change any other
document. Successful replacement SHALL ensure superseded chunks no longer appear in
keyword results.

When the incoming normalized deduplication hash equals the stored hash and canonical
chunks are present and consistent, the implementation MAY skip the canonical rewrite
and report `unchanged`. It SHALL still reconcile configured incomplete derived work.
Repeated refresh of the same item SHALL NOT duplicate documents, chunks, embeddings
or sidecar entries.

Targeted refresh SHALL NOT read, create, advance or rewrite connector checkpoints.
It SHALL NOT infer deletion from an absent item or an empty body.

## Derived work and result model

Embedding calls and sidecar updates SHALL begin only after the item's canonical
transaction commits, or after an item is confirmed unchanged. They SHALL use the same
configured provider, model identity, chunk hashes and vector-index behavior as full
sync. A derived failure SHALL NOT roll back canonical state or affect unrelated
documents.

The ordered report SHALL contain one result for every requested item. Each result
SHALL identify the `source_id` and expose these independent states:

- `canonical`: `indexed`, `unchanged`, `rejected`, `not_attempted`, or `failed`;
- `embedding`: `not_configured`, `current`, `pending`, `failed`, or
  `not_attempted`; and
- `sidecar`: `not_configured`, `current`, `pending`, `failed`, or `not_attempted`.

`indexed` means the canonical transaction committed. `unchanged` means the existing
canonical representation already matched. `rejected` is limited to preflight or
extraction rejection. `failed` means the item's canonical storage operation failed.

`not_configured` means that derived facility is disabled. `current` means it is
complete for all current chunks. `pending` means retryable work remains, including an
unavailable optional embedding provider or a sidecar that can be rebuilt from
canonical SQLite. `failed` means attempted work ended in a non-retryable categorized
failure. `not_attempted` means canonical rejection/failure or whole-batch preflight
prevented the work.

A result SHALL include sanitized machine-readable reason codes for every
`rejected`, `pending` or `failed` state. It MAY include safe counts and document IDs.
It SHALL NOT include raw content, raw provider errors, response bodies, credentials or
secret references. Retrying an item with `pending` or `failed` derived state SHALL be
safe and SHALL re-evaluate current canonical hashes.

## Compatibility and exposure

The first release SHALL expose targeted refresh only as an additive public Rust
library API. It SHALL add no `ctx` command, HTTP or MCP method, resource, model tool,
configuration-triggered refresh, watcher or background worker.

Existing `ctx sync` discovery, filtering, progress, summaries and checkpoint behavior
SHALL remain unchanged. Full sync and targeted refresh SHALL share the same per-item
normalization, extraction, chunking, embedding and vector synchronization
implementation; provider-name or connector-name dispatch SHALL not be introduced in
the targeted executor.

## Acceptance Criteria

- Enrollment tests prove exact configured-source resolution and show that resolution
  performs no network, secret, database, model/provider or process operation.
- Preflight tests cover empty/oversized batches, aggregate overflow, duplicate IDs,
  mismatched sources, malformed metadata, raw/body conflicts, extraction limits and
  ordered rejection reporting with zero writes and provider calls.
- Create, update and unchanged tests prove stable identity, complete chunk
  replacement, disappearance of superseded keyword results and idempotency.
- Transaction fault tests prove prior canonical state survives failure between every
  document/chunk/FTS/vector mutation boundary.
- Checkpoint tests prove targeted refresh performs no checkpoint reads or writes.
- Disabled, successful, retryably unavailable and non-retryable embedding modes each
  produce the specified states without changing canonical success.
- Every configured sidecar mode covers success and rebuildable failure while unrelated
  documents and indexes remain unchanged.
- Full-sync regression tests prove existing connector, CLI, checkpoint, extraction,
  embedding and search behavior remains compatible through the shared ingestor.
