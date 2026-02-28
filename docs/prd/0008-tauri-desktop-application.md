# PRD-0008: Tauri Desktop Application

**Status:** Planned
**Date:** 2026-02-27
**Author:** pjones

## Problem Statement

Context Harness is a powerful CLI tool, but not all users are comfortable
with the terminal. Managing data sources, browsing indexed documents,
running searches, and configuring agents through CLI flags and TOML files
creates friction for users who prefer graphical interfaces. There is no
way to visualize the knowledge base, inspect individual documents, or
tune search parameters interactively.

Additionally, some workflows benefit from a persistent desktop application:
always-on background sync, system tray access, real-time search, and
drag-and-drop document ingestion.

## Target Users

1. **Non-CLI users** who want Context Harness capabilities through a
   graphical interface.
2. **Power users** who want a persistent desktop companion for managing
   their knowledge base alongside their IDE.
3. **Teams** who want to deploy Context Harness to members who may not
   be comfortable with terminal tools.
4. **Demo audiences** who want to see Context Harness capabilities in
   an interactive, visual format.

## Goals

1. Build a native desktop application using Tauri (Rust backend + web
   frontend) that provides a graphical interface to Context Harness.
2. Depend on `context-harness-core` (and optionally the full
   `context-harness` crate) for all backend logic -- no reimplementation
   of search, chunking, or storage.
3. Provide core workflows: manage data sources, trigger sync, browse
   documents, search the knowledge base, configure embedding and
   retrieval settings.
4. Support background operation with system tray presence for always-on
   sync and search.
5. Target macOS, Linux, and Windows via Tauri's cross-platform support.

## Non-Goals

- Replacing the CLI for advanced users and automation.
- Building a web application (the desktop app uses web technologies for
  the frontend but runs natively).
- Implementing a chat interface with LLM integration (that can be added
  later or via PRD-0007's WASM approach).
- Mobile support (iOS, Android).

## User Stories

**US-1: Browse the knowledge base.**
A user opens the Tauri app and sees a dashboard showing: source count,
document count, embedding coverage, and last sync time. They click into
a source and browse its documents. They click a document and see its
full content, metadata, and chunks.

**US-2: Interactive search.**
A user types a query in the search bar. Results appear in real-time
with relevance scores. They toggle between keyword, semantic, and hybrid
modes. They adjust the hybrid alpha slider and see results re-rank
instantly.

**US-3: Manage data sources.**
A user clicks "Add Source" and configures a Git connector by entering a
repo URL and include globs. They click "Sync" and see a progress bar.
The app writes the connector config to `ctx.toml` and invokes sync.

**US-4: System tray operation.**
A user closes the app window. The app minimizes to the system tray.
Background sync continues on a schedule. The tray icon shows sync
status. Clicking the tray icon opens a quick search popup.

**US-5: Drag-and-drop ingestion.**
A user drags a folder of Markdown files onto the app window. The app
creates a filesystem connector entry and syncs the directory.

## Requirements

1. The application SHALL use Tauri 2.x with a web frontend (framework
   TBD: could be vanilla, Svelte, React, or similar).
2. The Rust backend SHALL depend on `context-harness-core` and/or
   `context-harness` for all search, storage, and ingestion logic.
3. The application SHALL provide views for: dashboard/overview, source
   management, document browser, search, and settings.
4. Search SHALL support all three modes (keyword, semantic, hybrid)
   with interactive alpha adjustment.
5. The application SHALL support background operation via system tray
   on all three platforms.
6. The application SHALL read and write `ctx.toml` for configuration,
   maintaining compatibility with the CLI.
7. The application SHALL display sync progress with per-source status.

## Success Criteria

- A non-CLI user can install the app, configure a source, sync, and
  search without touching the terminal.
- The app starts in <3 seconds on consumer hardware.
- Search results match CLI output exactly (same engine, same scoring).
- The app runs on macOS, Linux (AppImage or deb), and Windows (MSI).
- Config files produced by the app are valid for the CLI and vice versa.

## Dependencies and Risks

- **PRD-0006 (Workspace Refactor):** The Tauri app benefits from
  depending on `context-harness-core` for a lighter dependency tree.
  Without the refactor, it must depend on the full `context-harness`
  crate, pulling in server and CLI dependencies that are unnecessary.
- **Frontend framework choice:** Tauri supports any web framework for
  the frontend. The choice affects developer experience and bundle size.
  Decision deferred to implementation.
- **System tray behavior:** Platform-specific differences in tray
  behavior (especially Linux with multiple desktop environments) may
  require per-platform testing.
- **Config file conflicts:** If the CLI and Tauri app both modify
  `ctx.toml`, file locking or conflict resolution may be needed.
- **SQLite locking:** If the CLI and Tauri app access the same SQLite
  database, WAL mode handles concurrent reads but only one writer at a
  time. This is acceptable for the target use case.

## Related Documents

- **PRDs:** [PRD-0006](0006-workspace-refactor-and-library-publishing.md)
  (prerequisite: workspace split enables lightweight dependency),
  [PRD-0001](0001-core-context-engine.md) (core engine the app wraps)
- **ADRs:** [0018](../adr/0018-store-abstraction-and-workspace-split.md)
  (workspace split motivated in part by Tauri),
  [0002](../adr/0002-sqlite-as-embedded-storage.md) (SQLite shared
  between CLI and desktop app)
