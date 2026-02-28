# ADR-0006: Paragraph-Boundary Chunking

**Status:** Accepted
**Date:** Retroactive

## Context

Ingested documents must be split into smaller chunks for embedding and
retrieval. Chunk size affects both search quality and embedding cost:

- Too large: embeddings are diluted across topics, reducing retrieval
  precision; exceeds model context windows
- Too small: fragments lose context, increasing the number of embeddings
  and producing noisy search results
- Splitting mid-sentence or mid-paragraph breaks semantic coherence

The chunking strategy must work across all content types ingested by
Context Harness: Markdown documentation, plain text, code, PDF-extracted
text, and structured data from Lua connectors.

## Decision

Use **paragraph-boundary chunking** that splits on `\n\n` (double newline)
boundaries.

Parameters (configurable in `[chunking]`):

| Parameter | Default | Purpose |
|-----------|---------|---------|
| `max_tokens` | 700 | Maximum tokens per chunk (estimated as chars / 4) |
| `overlap_tokens` | 80 | Overlap between consecutive chunks for context continuity |

Algorithm:

1. Split the document body on `\n\n` into paragraphs.
2. Accumulate paragraphs into a chunk until adding the next paragraph would
   exceed `max_tokens * 4` characters.
3. When the limit is reached, emit the current chunk and start a new one
   with `overlap_tokens * 4` characters carried over from the end of the
   previous chunk.
4. If a single paragraph exceeds the max size, split it further at newline
   or space boundaries.
5. Compute a SHA-256 hash of each chunk's text for staleness detection
   (used by the embedding system to avoid re-embedding unchanged chunks).
6. Assign each chunk a UUID v4 identifier.

## Alternatives Considered

**Sentence-level splitting.** Libraries like `unicode-segmentation` or
`nltk` split on sentence boundaries. Produces very small chunks that lack
context — a single sentence rarely contains enough information for meaningful
retrieval. Would also dramatically increase the number of embeddings.

**Fixed-size character/token splitting.** Split every N characters regardless
of content structure. Simple but frequently cuts mid-paragraph or mid-sentence,
producing chunks with broken context. Common in naive RAG implementations but
known to degrade retrieval quality.

**Recursive / AST-aware splitting.** Parse documents into a tree (headings,
sections, code blocks) and split along structural boundaries. More
sophisticated but adds significant complexity — requires per-format parsers
(Markdown, code, PDF) and produces inconsistent chunk sizes. The marginal
quality gain does not justify the complexity at the current scale.

**Sliding window without paragraph alignment.** Overlapping fixed-size windows.
Captures context across boundaries but ignores document structure entirely.
Paragraph-aligned chunks are more coherent for both search and human reading.

## Consequences

- Chunks align with natural document structure (paragraphs), preserving
  semantic coherence. This produces better embeddings and more useful
  search snippets.
- The overlap parameter provides context continuity across chunk boundaries,
  reducing the chance of missing relevant content that spans two chunks.
- The token estimate (chars / 4) is approximate. This is acceptable because
  chunk size does not need to be exact — it is a soft limit for embedding
  model input windows, not a hard protocol constraint.
- SHA-256 hashing per chunk enables the embedding system to detect unchanged
  chunks and skip re-embedding during incremental sync, reducing API costs
  and sync time.
- The strategy works well for prose-heavy content (docs, articles, runbooks)
  but is suboptimal for highly structured content like tables or code.
  This is a known tradeoff accepted in favor of simplicity.
