# PRD: Context Harness Native App

**Status:** Planning  
**Type:** Product Requirements Document  
**Date:** 2026-02-28  
**Depends on:** [ADR-0020](../adr/0020-cross-platform-framework-selection.md) (Tauri 2.0), [SPEC-0002](../spec/0002-workspace-refactor.md) (core crate)

---

## 1. Product vision

Context Harness is a local-first context ingestion and retrieval framework. Today it is accessed via a CLI (`ctx`) and an MCP HTTP server consumed by AI tools like Cursor and Claude Desktop. The native app extends this into a standalone product: a visual interface for building, managing, and querying personal and team knowledge bases.

**Positioning:** "Your knowledge base, on your device." The app makes it easy to connect sources, ingest content, search across everything, and manage the extensions that power it -- without touching a terminal.

The app is a standalone product. It shares `context-harness-core` and `context-harness` as library dependencies but has its own identity, its own release cadence, and its own audience. It complements the CLI rather than replacing it.

---

## 2. User personas

### 2.1 Individual developer ("Alex")

Alex is a senior developer who works across multiple projects and accumulates knowledge in Git repos, Confluence, Notion, local markdown files, and Slack threads. Alex uses Cursor daily and wants an AI assistant grounded in their actual documentation, not generic training data.

**Goals:**
- Quickly set up a knowledge base that indexes project docs, runbooks, and code context.
- Search across all sources from one place.
- Keep the MCP server running so Cursor can access the knowledge base.
- Install community connectors (Jira, Slack, RSS) without writing config files by hand.

**Pain points with CLI-only:**
- Editing TOML config for connectors is tedious and error-prone.
- No visual feedback during sync -- just terminal output.
- Switching between workspaces requires remembering paths and config flags.

### 2.2 Team lead ("Jordan")

Jordan manages a platform team. They want to set up a shared knowledge base that indexes architecture decision records, runbooks, API specs, and onboarding guides so the team's AI tools are grounded in the team's actual standards.

**Goals:**
- Configure connectors for team Git repos, Confluence spaces, and Jira projects.
- Monitor sync health (are connectors up to date? any failures?).
- Browse and verify what's been indexed before exposing it to the team.
- Share the workspace config so team members can replicate it.

**Pain points with CLI-only:**
- No dashboard to see connector health at a glance.
- Hard to verify indexed content without running search queries from the terminal.
- Onboarding team members to the CLI and config format takes time.

---

## 3. Core feature areas

### 3.1 Workspace management

A **workspace** is a self-contained knowledge base: a configuration file (`ctx.toml`), a SQLite database, and associated connectors, tools, and agents.

- **Create workspace:** Guided flow to create a new workspace (name, location, embedding provider).
- **Open workspace:** Open an existing workspace from disk. The app remembers recently opened workspaces.
- **Workspace dashboard:** At-a-glance view of the active workspace: document count, connector status, embedding status, last sync time, MCP server state.
- **Workspace settings:** Edit workspace configuration (chunking, retrieval tuning, server bind address) through a visual form rather than raw TOML.
- **Multi-workspace support:** Switch between workspaces within the app. Each workspace is independent.

### 3.2 Connector configuration

Connectors are the data sources that feed a workspace.

- **Add connector:** Visual form for each connector type (filesystem, Git, S3, script). Fields are type-specific: filesystem shows root path and glob patterns; Git shows URL, branch, and clone options; S3 shows bucket, prefix, and region.
- **Edit connector:** Modify an existing connector's configuration.
- **Remove connector:** Remove a connector and optionally purge its indexed documents.
- **Test connector:** Validate connector configuration without running a full sync (equivalent to `ctx connector test`).
- **Connector status:** Per-connector health indicator (last sync time, document count, errors).

### 3.3 Sync and ingest

- **Trigger sync:** Start a sync for all connectors or a specific connector. Equivalent to `ctx sync [target]`.
- **Real-time progress:** Stream sync progress in the UI: connector name, documents processed, current file, elapsed time. Mirrors the progress reporting defined in [DESIGN-0002](../design/0002-sync-progress.md).
- **Sync history:** Log of recent syncs with timestamp, connector, documents added/updated/unchanged, duration, and errors.
- **Background sync:** Option to sync on a schedule or on workspace open (configurable).

### 3.4 Search

- **Search bar:** Prominent, always-accessible search input.
- **Mode selector:** Toggle between keyword, semantic, and hybrid search modes.
- **Filters:** Filter by source (connector name), date range, or document metadata.
- **Results list:** Ranked results with score, title, source, snippet, and last-updated date. Matches the shape of `SearchResultItem` from [SPEC-0006](../spec/0006-json-schemas.md).
- **Document preview:** Click a result to view the full document in a reading pane (rendered markdown, plain text, or extracted text for PDFs).
- **Score explanation:** Optional toggle to show score breakdown (keyword score, vector score, hybrid weight) per result.

### 3.5 Document browser

- **Source tree:** Browse indexed documents grouped by connector / source.
- **Document detail:** View a document's full body, metadata (source, source_id, updated_at, source_url), and chunk breakdown.
- **Chunk inspector:** View individual chunks with their embedding status and content hash.

### 3.6 Embedding management

- **Provider configuration:** Select embedding provider (local/fastembed, local/tract, OpenAI, Ollama, disabled) and model. Visual form for provider-specific settings (API key, endpoint, dimensions).
- **Embedding status:** Dashboard showing total chunks, embedded chunks, pending chunks, and staleness.
- **Embed pending:** Trigger embedding for pending chunks (equivalent to `ctx embed pending`).
- **Rebuild embeddings:** Re-embed all chunks with a new model or provider (equivalent to `ctx embed rebuild`). Progress indicator for long-running rebuilds.

### 3.7 Registry browser

- **Browse registry:** List available extensions from configured registries (connectors, tools, agents) with name, description, and type.
- **Search registry:** Filter extensions by keyword or type.
- **Extension detail:** View an extension's README, configuration schema, and source.
- **Install extension:** Add an extension to the workspace with a guided configuration form.
- **Update extensions:** Check for registry updates and apply them.
- **Manage registries:** Add, remove, or refresh extension registries.

### 3.8 Agent management

- **Agent list:** View all configured agents (inline TOML and Lua script) with name, description, tools, and source type.
- **Agent detail:** View an agent's system prompt, tool list, and arguments.
- **Test agent:** Resolve an agent's prompt with sample arguments and view the result (equivalent to `ctx agent test`).
- **Add agent:** Create a new inline agent via form or scaffold a Lua agent script.

### 3.9 MCP server control

- **Start/stop server:** Toggle the MCP HTTP server on or off from the app.
- **Server status:** Show bind address, uptime, and connection count.
- **Connection info:** Display the MCP endpoint URL and a copyable snippet for adding to `.cursor/mcp.json` or Claude Desktop config.
- **Tool list:** View the tools currently exposed by the server (built-in + script tools).

### 3.10 Settings

- **Appearance:** Light/dark theme, font size.
- **Default embedding provider:** Set the default provider for new workspaces.
- **Auto-update:** Enable or disable app auto-updates.
- **Data:** Clear app data, reset recently-opened workspaces.

---

## 4. Phasing

### Phase 1 -- MVP

**Goal:** A usable app that replaces the most common CLI workflows with a visual interface.

**Features:**
- Workspace management (create, open, switch, dashboard, settings)
- Connector configuration (add, edit, remove, status for filesystem and Git)
- Sync (trigger, real-time progress, history)
- Search (search bar, mode selector, filters, results, document preview)
- Document browser (source tree, document detail)
- Embedding management (provider config, status, embed pending)
- Settings (theme, defaults)

**Platforms:** macOS, Windows, Linux (desktop only).

**Prerequisites:** Workspace refactor ([SPEC-0002](../spec/0002-workspace-refactor.md)) must be complete so the app can depend on `context-harness-core` and `context-harness`.

### Phase 2 -- Extensions and agents

**Goal:** Full extension and agent lifecycle management in the app.

**Features:**
- Registry browser (browse, search, install, update)
- Agent management (list, detail, test, add)
- MCP server control (start/stop, status, connection info)
- S3 connector support in the connector configuration UI
- Script connector support (with Lua editor or file picker)
- Sync scheduling (background sync on interval or workspace open)

### Phase 3 -- Team and collaboration

**Goal:** Enable team workflows and shared knowledge bases.

**Features:**
- Workspace export/import (share workspace config without the database)
- Workspace templates (pre-configured connector sets for common team setups)
- Multi-user awareness (read-only view of who last synced, optional metadata)
- Mobile support (iOS, Android) via Tauri mobile targets

### Future considerations (not committed)

- In-app chat with RAG (integrate the agent loop from the WASM design into the native app)
- Connector authoring (visual Lua script editor for custom connectors)
- Background service mode (tray icon, persistent MCP server independent of the app window)
- Plugin system for third-party UI extensions

---

## 5. Non-goals (MVP)

The following are explicitly out of scope for Phase 1:

- **In-app chat or LLM inference.** The WASM demo app handles browser-based chat. The native app may integrate chat in a future phase, but the MVP focuses on management and search.
- **Building custom connectors in-app.** Users create Lua scripts externally; the app configures and runs them.
- **Running as a background service.** The MCP server runs while the app is open. A persistent background service is a future consideration.
- **Mobile.** Phase 1 targets desktop only. Mobile is architecturally supported by Tauri 2.0 and planned for Phase 3.
- **Team collaboration features.** Phase 1 is single-user. Team features are Phase 3.
- **Self-hosting or cloud deployment.** The app is local-first. Cloud sync or hosted workspaces are not planned.

---

## 6. Success metrics

### Phase 1

- A developer can create a workspace, add a filesystem and Git connector, sync, and search -- all without touching the terminal or editing TOML by hand.
- Sync progress is visible in real-time and matches the CLI's output fidelity.
- Search results match the CLI and MCP server output for the same query and workspace.
- App binary is under 15 MB (macOS). Memory usage is under 100 MB idle.
- App starts in under 2 seconds on a modern machine.

### Phase 2

- A developer can discover, install, and configure a registry extension (e.g., the Jira connector) entirely within the app.
- The MCP server can be started from the app and used by Cursor without any manual configuration beyond copying the endpoint URL.

---

## 7. User flows

### 7.1 First-run experience

1. User launches the app for the first time.
2. App shows a welcome screen with two options: "Create a new workspace" or "Open an existing workspace."
3. **Create:** User provides a name and location. App creates the directory, initializes the database, and generates a default `ctx.toml`. User lands on the workspace dashboard.
4. **Open:** User selects a directory containing an existing `ctx.toml`. App validates the config and opens the workspace dashboard.

### 7.2 Adding a Git connector and syncing

1. From the workspace dashboard, user clicks "Add connector."
2. User selects "Git" from the connector type picker.
3. User fills in the form: repository URL, branch (defaults to `main`), root path (optional), include/exclude globs (optional), shallow clone toggle.
4. User clicks "Save." The connector appears in the connector list with a "Never synced" status.
5. User clicks "Sync" (or "Sync all" from the dashboard).
6. The sync progress panel opens, showing real-time progress: cloning the repository, scanning files, processing documents.
7. On completion, the dashboard updates with the new document count and last sync time.

### 7.3 Searching and reading a document

1. User types a query in the search bar.
2. Results appear in real-time (or on submit) with ranked snippets.
3. User clicks a result. The document detail view opens in a side panel or full view.
4. The document body is rendered (markdown is formatted, plain text is displayed, PDF-extracted text is shown).
5. User can navigate to the chunk view to see how the document was split and which chunks have embeddings.

---

## 8. Dependencies

| Dependency | Status | Notes |
|------------|--------|-------|
| Workspace refactor ([SPEC-0002](../spec/0002-workspace-refactor.md)) | Planned | Required for the app to depend on `context-harness-core`. |
| ADR framework selection ([ADR-0020](../adr/0020-cross-platform-framework-selection.md)) | Accepted | Tauri 2.0 selected. |
| Technical spec ([SPEC-0001](../spec/0001-native-app.md)) | Draft | Architecture and implementation contracts. |

---

## 9. References

- [ADR-0020](../adr/0020-cross-platform-framework-selection.md) -- Framework decision (Tauri 2.0).
- [SPEC-0002](../spec/0002-workspace-refactor.md) -- Core/app crate split.
- [SPEC-0005](../spec/0005-usage-contract.md) -- CLI command reference (the app provides visual equivalents).
- [SPEC-0006](../spec/0006-json-schemas.md) -- API response shapes (search results, document responses).
- [DESIGN-0002](../design/0002-sync-progress.md) -- Sync progress reporting behavior.
- [SPEC-0011](../spec/0011-mcp-agents.md) -- Agent system design.
- [SPEC-0007](../spec/0007-extension-registries.md) -- Extension registry design.
