# PRD-0002: Browser Search Widget

**Status:** Delivered
**Date:** 2026-02-21 (conceived) / 2026-02-27 (documented)
**Author:** pjones

## Problem Statement

Documentation sites need search. The standard options -- Algolia, DocSearch,
or custom Elasticsearch -- require external services, ongoing costs, and
vendor lock-in. Teams that already index their docs with Context Harness
have a rich, searchable dataset sitting in SQLite. There is no way to
surface that same index as a browser-based search experience on a static
site without standing up a server.

## Target Users

1. **Documentation site maintainers** who want instant search without
   third-party services or monthly bills.
2. **Open-source projects** that publish docs to GitHub Pages or similar
   static hosting and want a search widget.
3. **Context Harness users** who want to dogfood the same index that
   powers their MCP server as a public search interface.

## Goals

1. Provide a drop-in JavaScript search widget (`ctx-search.js`) that adds
   a command-K search overlay to any static site.
2. Support both keyword (BM25) and semantic search in the browser using
   a pre-built JSON export from the Context Harness index.
3. Enable a CI pipeline (`build-docs.sh`) that indexes documentation and
   exports the JSON data file on every push.
4. Run entirely in the browser -- no backend server required for the
   search widget itself.
5. Support optional browser-side semantic search via Transformers.js for
   sites that want it.

## Non-Goals

- Replacing the MCP server for AI tool integration (that is PRD-0001).
- Full RAG or chat in the browser (that is PRD-0007).
- Server-side rendering or dynamic search APIs.

## User Stories

**US-1: Add search to a docs site.**
A maintainer adds a single `<script>` tag pointing to `ctx-search.js` and
a `data-json` attribute pointing to their exported index. Users press
Cmd+K and get instant search results.

**US-2: CI-powered index.**
A GitHub Action runs `ctx sync filesystem --full && ctx export` on every
push to `main`. The resulting `data.json` is deployed alongside the static
site. Search results update with every docs change.

**US-3: Semantic search in the browser.**
A site opts into browser-side embeddings via Transformers.js. Users get
semantic search results without any API keys or backend servers.

## Requirements

1. The widget SHALL render a command-K search overlay with keyboard
   navigation and result previews.
2. The widget SHALL load a JSON data file exported by `ctx export` and
   perform keyword search client-side.
3. `ctx export` SHALL produce a JSON file containing document metadata,
   chunk text, and optionally pre-computed embeddings.
4. The widget SHALL support optional semantic search via Transformers.js
   when embeddings are available.
5. The widget SHALL work on any static hosting platform (GitHub Pages,
   Netlify, Vercel, S3) with no server-side requirements.
6. The CI pipeline SHALL be documented with a GitHub Actions template.

## Success Criteria

- A docs site can add search with one `<script>` tag and one JSON file.
- Search results appear in <100ms for a typical documentation corpus
  (~500 pages).
- The widget works without any external API calls.
- The documentation site for Context Harness itself uses `ctx-search.js`
  (dogfooding).

## Dependencies and Risks

- **JSON export size:** Large corpora may produce large JSON files. The
  export should support excluding embeddings to reduce size.
- **Transformers.js model loading:** First-load latency for semantic search
  can be significant. The widget should degrade gracefully to keyword-only
  while the model loads.

## Related Documents

- **PRDs:** [PRD-0001](0001-core-context-engine.md) (core engine that
  produces the index), [PRD-0007](0007-wasm-client-and-browser-rag.md)
  (future: full browser RAG supersedes the simple widget)
- **Specs:** [USAGE.md](../USAGE.md) (`ctx export` command),
  [DEPLOYMENT.md](../DEPLOYMENT.md) (CI pipeline documentation)
