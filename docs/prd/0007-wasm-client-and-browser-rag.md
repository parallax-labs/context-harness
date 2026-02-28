# PRD-0007: WASM Client and Browser RAG

**Status:** Planned
**Date:** 2026-02-27
**Author:** pjones

## Problem Statement

Context Harness currently requires a native binary and a running server
for search. This excludes several compelling use cases:

1. **Static site "chat with docs."** Documentation sites want to let
   visitors ask questions and get answers from the docs -- without
   standing up a backend server. Today, the browser search widget
   (PRD-0002) handles keyword search, but full RAG (semantic search +
   chat with tool calling) is not possible.

2. **Offline demos and kiosks.** Showcasing Context Harness at
   conferences or in sales demos requires a server. A fully in-browser
   version eliminates setup friction.

3. **Privacy-sensitive data.** Some data must never leave the device.
   A WASM-based RAG pipeline keeps everything in the browser: ingestion,
   chunking, embedding, search, and even the chat LLM.

The workspace refactor (PRD-0006) creates `context-harness-core` which
compiles to `wasm32-unknown-unknown`. This PRD builds on that foundation
to deliver a full in-browser RAG experience.

## Target Users

1. **Documentation site operators** who want "chat with your docs" on a
   static site with zero backend.
2. **Evaluators and demo audiences** who want to try Context Harness
   without installing anything.
3. **Privacy-conscious users** who need all processing to happen
   on-device in the browser.
4. **Open-source project maintainers** who want an interactive docs
   experience hosted for free on GitHub Pages.

## Goals

1. Compile `context-harness-core` to WASM and expose chunking, tract-based
   embedding, in-memory storage, and search as WASM exports.
2. Build a JavaScript agent loop that uses Transformers.js for the chat
   LLM and calls WASM for tool execution (search, get).
3. Ship a static demo app hostable on GitHub Pages that lets users upload
   documents, ask questions, and see RAG in action.
4. Persist conversation history in IndexedDB so returning users see
   prior threads.

## Non-Goals

- Replacing the native CLI/MCP server for production use.
- Running the full application (connectors, config, Lua runtime) in WASM.
- Supporting filesystem, Git, or S3 connectors in the browser.
- Matching native performance for large corpora.

## User Stories

**US-1: Chat with uploaded docs.**
A user visits the demo site, uploads three Markdown files, and asks
"What are the main architecture decisions?" The system chunks the files,
embeds them with tract in WASM, and the chat agent (Transformers.js)
searches the in-memory index and generates an answer with citations.
No server is contacted.

**US-2: Persistent conversations.**
A user returns to the demo the next day. Their previous conversation
threads are listed in a sidebar, loaded from IndexedDB. They can
continue a prior thread or start a new one.

**US-3: Embed on a docs site.**
A project embeds the WASM-powered search and chat on their documentation
site. Visitors can ask questions about the docs directly on the site.
The pre-built WASM binary is loaded from a CDN or hosted alongside the
site.

**US-4: Fully offline demo.**
A developer downloads the demo app assets. They open `index.html` locally
(or serve via `python -m http.server`). Everything works offline after
the initial model download.

## Requirements

### WASM Module

1. A `context-harness-wasm` crate SHALL depend only on
   `context-harness-core` and `wasm-bindgen`.
2. The WASM module SHALL expose: `add_document(title, body, metadata)`,
   `search(query, mode, limit)`, and `get(document_id)`.
3. Chunking SHALL use the same paragraph-boundary algorithm as the native
   build.
4. Embedding SHALL use the tract-based pipeline compiled to WASM, with
   the same ONNX models and dimensions as desktop.
5. Storage SHALL be in-memory (Rust structs inside the WASM module).
   Documents, chunks, and vectors do not persist across page reloads
   in MVP.

### Agent Loop

6. The agent loop SHALL run in JavaScript, not in WASM.
7. The chat LLM SHALL use Transformers.js (e.g., SmolLM, Phi) for text
   generation. Transformers.js is used only for the LLM, not for
   embeddings.
8. Tool calling SHALL use a prompt-based format (e.g.,
   `<tool_call>...</tool_call>`) parsed by the JS agent loop.
9. Tool execution (search, get) SHALL call WASM exports and return
   results to the agent loop.

### Demo App

10. The demo app SHALL be a static site deployable to GitHub Pages.
11. The demo app SHALL support file upload as the primary document
    input method.
12. URL-based document input MAY be supported as a secondary method,
    with documented CORS limitations.
13. Conversation history SHALL be persisted in IndexedDB with a schema
    supporting multiple threads.
14. The UI SHALL show loading states for: WASM initialization, model
    loading, document processing, and chat generation.

## Success Criteria

- The WASM binary loads and initializes in <5 seconds on a modern
  browser (Chrome/Firefox/Safari).
- A user can upload 10 Markdown files, ask a question, and receive a
  RAG-grounded answer within 30 seconds.
- The demo app is hostable on GitHub Pages with zero server-side
  components.
- Conversation history survives page reloads.
- The same embedding model produces consistent results in WASM and
  native builds.

## Dependencies and Risks

- **PRD-0006 (Workspace Refactor):** This PRD depends entirely on the
  workspace split. `context-harness-core` must exist and compile to
  WASM before this work can begin.
- **WASM binary size:** The tract ONNX model plus the WASM binary may
  be 20-80MB. This affects first-load latency. Mitigation: lazy loading,
  progress indicators, and potentially streaming model download.
- **Transformers.js model size:** Chat models (SmolLM, Phi) are large
  (hundreds of MB). First-load latency is significant. Document expected
  sizes and provide a clear loading experience.
- **Browser compatibility:** WASM and WebWorkers are widely supported
  but edge cases exist. Test on Chrome, Firefox, Safari.
- **Quality of small chat models:** Small local LLMs may produce lower-
  quality responses than cloud models. This is acceptable for a demo;
  document the tradeoff.

## Related Documents

- **Design:** Context Harness WASM Client Design (Obsidian),
  Context Harness WASM Demo App Design (Obsidian)
- **PRDs:** [PRD-0002](0002-browser-search-widget.md) (predecessor:
  browser search widget), [PRD-0006](0006-workspace-refactor-and-library-publishing.md)
  (prerequisite: workspace split)
- **ADRs:** [0018](../adr/0018-store-abstraction-and-workspace-split.md)
  (Store trait enables in-memory store for WASM)
