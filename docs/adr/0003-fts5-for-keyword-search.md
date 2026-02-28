# ADR-0003: FTS5 for Keyword Search

**Status:** Accepted
**Date:** Retroactive

## Context

Keyword search is a core retrieval mode in Context Harness. Users need to
find documents by searching for specific terms, phrases, or combinations of
words across their ingested corpus. The search engine must:

- Rank results by relevance (not just filter)
- Handle common text search expectations (stemming-aware ranking, term
  frequency weighting)
- Integrate with the existing SQLite storage layer without adding external
  dependencies
- Support the hybrid search pipeline where keyword scores are combined with
  semantic scores

## Decision

Use **SQLite FTS5** with **BM25 ranking** for keyword search.

A virtual table `chunks_fts` is created over the `text` column of the
`chunks` table. Keyword queries are executed against this FTS5 table, and
results are ranked using FTS5's built-in `bm25()` function.

BM25 scores from FTS5 are negative (lower is better). The search pipeline
normalizes these via min-max normalization to `[0, 1]` before combining with
semantic scores in hybrid mode (see
[ADR-0005](0005-hybrid-scoring-with-min-max-normalization.md)).

The FTS5 index is maintained automatically by SQLite as chunks are inserted
or replaced during sync.

## Alternatives Considered

**Tantivy.** A Rust-native full-text search library inspired by Lucene.
Powerful and fast, but introduces a separate index that must be kept in sync
with SQLite. This adds complexity (two sources of truth for chunk text) and
a second storage location to manage. FTS5 avoids this by living inside the
same SQLite database.

**Elasticsearch / Meilisearch / Typesense.** External search services with
rich features, but each requires a running server process. This violates the
local-first, zero-dependency design. The search quality of FTS5 with BM25 is
sufficient for personal-scale corpora.

**Custom inverted index.** Building a purpose-built index in Rust would allow
maximum control but is significant engineering effort for marginal benefit
over FTS5, which is battle-tested and ships with SQLite.

**No keyword search (semantic only).** Semantic search alone misses exact-match
queries (error codes, function names, specific terms) where keyword search
excels. Both modes are needed for comprehensive retrieval.

## Consequences

- Zero additional dependencies — FTS5 is compiled into SQLite and available
  via `sqlx` without feature flags or extensions.
- The FTS5 index is transactionally consistent with the `chunks` table since
  both live in the same database.
- BM25 ranking provides good-enough relevance for the target corpus sizes
  without tuning. Advanced features like custom tokenizers or field boosting
  are available in FTS5 if needed later.
- FTS5 does not support fuzzy matching or typo tolerance. Users must search
  for exact terms (though BM25 handles partial term frequency well).
- The negative-score convention of FTS5's `bm25()` requires normalization
  before combining with semantic scores, adding a small amount of complexity
  to the hybrid pipeline.
