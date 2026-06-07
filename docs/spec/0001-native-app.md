# Native App Spec

**Status:** Draft -- not yet implemented  
**Scope:** Architecture, crate structure, IPC contract, frontend choice, and platform packaging for the Context Harness native desktop app.  
**Policy:** Implementation SHALL conform to this spec once status changes to Authoritative. While in Draft status, this document defines the intended architecture and contracts; it MAY be updated as design decisions are finalized during implementation.

**Summary.** The native app is a Tauri 2.0 application that provides a visual interface for Context Harness. The Rust backend depends directly on `context-harness-core` and `context-harness` as workspace crates. The frontend is a TypeScript SPA rendered in the system webview. The app manages workspaces, configures connectors, runs sync, provides search and document browsing, and controls the MCP server.

---

## 1. Definitions

- **App** -- The Tauri application: Rust backend + TypeScript frontend + Tauri IPC bridge.
- **Core** -- The `context-harness-core` crate (models, chunk, Store trait, search algorithm, tract embeddings). Defined in [SPEC-0002](0002-workspace-refactor.md).
- **Library** -- The `context-harness` crate (CLI, server, connectors, SQLite, config, ingest). Also defined in [SPEC-0002](0002-workspace-refactor.md).
- **Workspace** -- A user-facing knowledge base: a directory containing a `ctx.toml` config file and a SQLite database. One workspace is open at a time in the app.
- **Command** -- A Tauri IPC command: a Rust function annotated with `#[tauri::command]` that the frontend can invoke.
- **Event** -- A Tauri event emitted from the Rust backend and received by the frontend (e.g., sync progress updates).

---

## 2. Prerequisites

This spec depends on the workspace refactor defined in [SPEC-0002](0002-workspace-refactor.md). The app crate requires:

- `context-harness-core` exists as a separate crate with the Store trait, models, chunking, and search algorithm.
- `context-harness` exists as a separate crate with SqliteStore, connectors, config, ingest, embedding providers, server, and public API.

The app MAY be developed in parallel with the refactor by depending on `context-harness` directly (pre-refactor, single crate). Once the refactor is complete, the app's `Cargo.toml` SHALL be updated to depend on both `context-harness-core` and `context-harness`.

---

## 3. Crate structure

### 3.1 Workspace layout

The app SHALL be a new workspace member in the Context Harness repository.

```
context-harness/
  Cargo.toml                    # workspace root
  crates/
    context-harness-core/       # models, chunk, store, search, tract (from refactor)
    context-harness/            # CLI, server, connectors, SQLite, config (from refactor)
    context-harness-app/        # Tauri app (this spec)
      Cargo.toml
      tauri.conf.json
      capabilities/             # Tauri permission files
      icons/
      src/
        main.rs                 # Tauri entry point (prevents console window on Windows)
        lib.rs                  # App setup, state, command registration
        commands/               # IPC command modules
          mod.rs
          workspace.rs          # Workspace CRUD, open, list recent
          connector.rs          # Connector config CRUD, test
          sync.rs               # Sync trigger, progress events
          search.rs             # Search queries
          document.rs           # Document retrieval, browsing
          embedding.rs          # Embedding status, trigger embed
          registry.rs           # Registry browse, install, update
          agent.rs              # Agent list, detail, test
          server.rs             # MCP server start/stop, status
          settings.rs           # App-level settings
        state.rs                # Tauri managed state (AppState)
      frontend/                 # TypeScript SPA
        package.json
        tsconfig.json
        vite.config.ts
        src/
          main.ts
          App.svelte            # (or equivalent for chosen framework)
          lib/
            bindings.ts         # Auto-generated from tauri-specta
          ...
```

### 3.2 Cargo.toml

The app crate SHALL depend on the workspace crates and Tauri:

```toml
[package]
name = "context-harness-app"
version = "0.1.0"
edition = "2021"

[lib]
name = "context_harness_app"
crate-type = ["lib", "cdylib", "staticlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
context-harness-core = { path = "../context-harness-core" }
context-harness = { path = "../context-harness" }
tauri = { version = "2", features = [] }
tauri-plugin-shell = "2"
tauri-specta = { version = "2", features = ["derive", "typescript"] }
specta = { version = "2", features = ["derive"] }
specta-typescript = "0.0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

The root `Cargo.toml` SHALL include `"crates/context-harness-app"` in the workspace members list.

### 3.3 Binary

The app produces a single binary. On Windows, the entry point (`main.rs`) SHALL use `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` to suppress the console window in release builds.

---

## 4. Architecture

### 4.1 Component diagram

```
+-----------------------------------------------+
|                 System Webview                  |
|                                                 |
|  +-------------------------------------------+  |
|  |          TypeScript Frontend               |  |
|  |                                           |  |
|  |  Components <-> State <-> IPC Bindings    |  |
|  +-------------------------------------------+  |
|                     | invoke()                   |
|                     | listen()                   |
+-----------------------------------------------+
                      |
              Tauri IPC Bridge
              (JSON serialization)
                      |
+-----------------------------------------------+
|              Rust Backend                       |
|                                                 |
|  +----------+  +------------+  +-------------+ |
|  | Commands |->| AppState   |->| context-    | |
|  | (IPC)    |  | (workspace |  | harness lib | |
|  +----------+  |  config,   |  +------+------+ |
|                |  db pool)  |         |         |
|  +----------+  +------------+  +------+------+ |
|  | Events   |                  | context-    | |
|  | (progress|                  | harness-    | |
|  |  sync)   |                  | core        | |
|  +----------+                  +-------------+ |
+-----------------------------------------------+
                      |
              +-------+-------+
              |    SQLite     |
              | (workspace DB)|
              +---------------+
```

### 4.2 Managed state

The Tauri app SHALL manage a single `AppState` struct as Tauri managed state:

```rust
pub struct AppState {
    /// The currently open workspace, if any.
    /// Protected by a tokio RwLock because commands are async.
    pub workspace: tokio::sync::RwLock<Option<WorkspaceState>>,
    /// App-level settings (persisted separately from workspace config).
    pub settings: tokio::sync::RwLock<AppSettings>,
}

pub struct WorkspaceState {
    /// Loaded and validated Config from ctx.toml.
    pub config: Config,
    /// Path to the workspace directory (parent of ctx.toml).
    pub path: std::path::PathBuf,
    /// SQLite connection pool for this workspace's database.
    pub pool: sqlx::SqlitePool,
}
```

Commands access state via `tauri::State<AppState>`. When a workspace is opened, the app loads the config, opens the database pool, and stores them in `AppState.workspace`. When a workspace is closed or switched, the previous pool is closed.

### 4.3 Command execution model

All Tauri commands SHALL be `async fn` and SHALL run on the Tokio runtime. Long-running operations (sync, embedding rebuild) SHALL:

1. Spawn a background `tokio::task`.
2. Emit progress events to the frontend via `app_handle.emit(event_name, payload)`.
3. Return immediately with an operation ID that the frontend can use to track or cancel the operation.

This prevents the IPC bridge from blocking during multi-second operations.

---

## 5. IPC contract

### 5.1 Type-safe bindings

The app SHALL use `tauri-specta` to generate TypeScript type definitions from Rust command signatures. All command parameters and return types SHALL derive `specta::Type` and `serde::Serialize` / `serde::Deserialize`.

The generated bindings SHALL be written to `frontend/src/lib/bindings.ts` during the build step. The frontend SHALL import and use these bindings exclusively -- no hand-written `invoke()` calls with string command names.

### 5.2 Error handling

Commands SHALL return `Result<T, AppError>` where `AppError` is a serializable error type:

```rust
#[derive(Debug, thiserror::Error, serde::Serialize, specta::Type)]
pub enum AppError {
    #[error("No workspace is open")]
    NoWorkspace,
    #[error("Workspace not found: {0}")]
    WorkspaceNotFound(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Connector error: {0}")]
    ConnectorError(String),
    #[error("Search error: {0}")]
    SearchError(String),
    #[error("Embedding error: {0}")]
    EmbeddingError(String),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}
```

The frontend receives errors as structured objects and can display appropriate messages.

### 5.3 Command groups

The following sections define the command interface. Parameter and return types reference existing library types where applicable (e.g., `SearchResultItem`, `Config`). Exact Rust signatures are normative; the TypeScript bindings are generated.

#### 5.3.1 Workspace commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `workspace_create` | `name: String, path: String, embedding_provider: Option<String>` | `WorkspaceInfo` | Create a new workspace directory with default `ctx.toml` and initialized database. |
| `workspace_open` | `path: String` | `WorkspaceInfo` | Open an existing workspace. Loads config, opens DB pool, stores in AppState. |
| `workspace_close` | -- | `()` | Close the current workspace. Releases DB pool. |
| `workspace_get_info` | -- | `WorkspaceInfo` | Return info about the currently open workspace. |
| `workspace_list_recent` | -- | `Vec<RecentWorkspace>` | List recently opened workspaces (stored in app settings). |
| `workspace_get_config` | -- | `Config` | Return the full parsed config of the current workspace. |
| `workspace_update_config` | `config: Config` | `()` | Write updated config to `ctx.toml`. Reloads state. |

```rust
#[derive(Serialize, specta::Type)]
pub struct WorkspaceInfo {
    pub name: String,
    pub path: String,
    pub document_count: u64,
    pub chunk_count: u64,
    pub embedded_chunk_count: u64,
    pub last_sync: Option<String>,
    pub server_running: bool,
}
```

#### 5.3.2 Connector commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `connector_list` | -- | `Vec<ConnectorInfo>` | List all configured connectors with status. |
| `connector_add` | `connector_type: String, name: String, config: serde_json::Value` | `()` | Add a new connector to the workspace config. |
| `connector_update` | `connector_type: String, name: String, config: serde_json::Value` | `()` | Update an existing connector's config. |
| `connector_remove` | `connector_type: String, name: String, purge_documents: bool` | `()` | Remove a connector. Optionally delete its indexed documents. |
| `connector_test` | `connector_type: String, name: String` | `ConnectorTestResult` | Validate connector config without syncing. |

```rust
#[derive(Serialize, specta::Type)]
pub struct ConnectorInfo {
    pub name: String,
    pub connector_type: String,  // "filesystem", "git", "s3", "script"
    pub document_count: u64,
    pub last_sync: Option<String>,
    pub healthy: bool,
    pub notes: Option<String>,
}
```

#### 5.3.3 Sync commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `sync_start` | `target: Option<String>` | `String` | Start a sync. Target is `"all"`, `"git"`, `"git:name"`, etc. Returns an operation ID. |
| `sync_cancel` | `operation_id: String` | `()` | Cancel a running sync. |

Sync progress SHALL be emitted as Tauri events:

- **Event name:** `sync-progress`
- **Payload:**

```rust
#[derive(Serialize, Clone, specta::Type)]
pub struct SyncProgressEvent {
    pub operation_id: String,
    pub connector: String,
    pub phase: String,        // "scanning", "processing", "complete", "error"
    pub current: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
    pub elapsed_ms: u64,
    pub message: Option<String>,
}
```

#### 5.3.4 Search commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `search` | `query: String, mode: String, limit: Option<i64>, source: Option<String>, since: Option<String>, explain: Option<bool>` | `Vec<SearchResultItem>` | Execute a search. Mode is `"keyword"`, `"semantic"`, or `"hybrid"`. Returns the same `SearchResultItem` shape as the MCP server. |

The `SearchResultItem` type SHALL be re-exported from `context-harness-core` (after the workspace refactor) and SHALL derive `specta::Type` for binding generation. Until the refactor, the app SHALL define a compatible mirror type.

#### 5.3.5 Document commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `document_get` | `id: String` | `DocumentResponse` | Get a full document by UUID. Same shape as `context.get` MCP tool. |
| `document_list` | `source: Option<String>, limit: Option<i64>, offset: Option<i64>` | `DocumentListResponse` | List documents, optionally filtered by source. Paginated. |
| `document_chunks` | `document_id: String` | `Vec<ChunkInfo>` | Get chunks for a document with embedding status. |

```rust
#[derive(Serialize, specta::Type)]
pub struct ChunkInfo {
    pub id: String,
    pub index: u32,
    pub text: String,
    pub content_hash: String,
    pub has_embedding: bool,
}
```

#### 5.3.6 Embedding commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `embedding_status` | -- | `EmbeddingStatus` | Get embedding pipeline status. |
| `embedding_run_pending` | -- | `String` | Embed all pending chunks. Returns operation ID. Progress via events. |
| `embedding_rebuild` | -- | `String` | Re-embed all chunks. Returns operation ID. Progress via events. |
| `embedding_update_config` | `config: EmbeddingConfig` | `()` | Update the embedding provider config. |

```rust
#[derive(Serialize, specta::Type)]
pub struct EmbeddingStatus {
    pub provider: String,
    pub model: Option<String>,
    pub total_chunks: u64,
    pub embedded_chunks: u64,
    pub pending_chunks: u64,
    pub stale_chunks: u64,
}
```

#### 5.3.7 Registry commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `registry_list_extensions` | `registry_name: Option<String>, extension_type: Option<String>` | `Vec<RegistryExtension>` | List available extensions. |
| `registry_search` | `query: String` | `Vec<RegistryExtension>` | Search extensions by keyword. |
| `registry_install` | `registry_name: String, extension_name: String` | `()` | Install an extension into the workspace. |
| `registry_update` | `registry_name: Option<String>` | `()` | Update registry index. |

#### 5.3.8 Agent commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `agent_list` | -- | `Vec<AgentInfo>` | List all configured agents. Same shape as `GET /agents/list`. |
| `agent_get` | `name: String` | `AgentInfo` | Get a single agent's details. |
| `agent_test` | `name: String, args: serde_json::Value` | `AgentPrompt` | Resolve an agent's prompt with test arguments. |

`AgentInfo` and `AgentPrompt` SHALL be the types defined in [SPEC-0011](0011-mcp-agents.md).

#### 5.3.9 Server commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `server_start` | -- | `()` | Start the MCP HTTP server using the workspace's server config. |
| `server_stop` | -- | `()` | Stop the MCP server. |
| `server_status` | -- | `ServerStatus` | Get server status. |

```rust
#[derive(Serialize, specta::Type)]
pub struct ServerStatus {
    pub running: bool,
    pub bind_address: Option<String>,
    pub uptime_secs: Option<u64>,
}
```

#### 5.3.10 Settings commands

| Command | Parameters | Returns | Description |
|---------|------------|---------|-------------|
| `settings_get` | -- | `AppSettings` | Get app-level settings. |
| `settings_update` | `settings: AppSettings` | `()` | Update app-level settings. Persisted to disk. |

```rust
#[derive(Serialize, Deserialize, specta::Type)]
pub struct AppSettings {
    pub theme: String,           // "light", "dark", "system"
    pub recent_workspaces: Vec<RecentWorkspace>,
    pub default_embedding_provider: String,
    pub auto_update: bool,
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RecentWorkspace {
    pub name: String,
    pub path: String,
    pub last_opened: String,
}
```

App settings SHALL be stored in the platform-appropriate app data directory (e.g., `~/Library/Application Support/context-harness-app/settings.json` on macOS, `%APPDATA%\context-harness-app\settings.json` on Windows). Tauri's `app_data_dir()` SHALL be used to resolve this path.

---

## 6. Frontend

### 6.1 Framework selection

The frontend SHALL use **Svelte** (v5, runes mode) with **TypeScript** and **Vite** as the build tool.

**Rationale:**

- **Bundle size.** Svelte compiles to vanilla JS with no runtime. This aligns with Tauri's lightweight philosophy. A Svelte SPA produces significantly smaller bundles than React (which ships a runtime) or SolidJS (smaller community and ecosystem).
- **Reactivity.** Svelte's reactivity model maps naturally to the event-driven IPC pattern: commands return data, events push updates, and reactive state reflects changes without boilerplate.
- **Ecosystem.** Svelte has a mature component ecosystem, good TypeScript support, and active maintenance. SvelteKit is not needed (the app is a SPA, not SSR).
- **Developer experience.** Single-file components, minimal boilerplate, built-in transitions and animations.

### 6.2 Build integration

The frontend SHALL be built by Vite during `cargo tauri build` and `cargo tauri dev`. Tauri's `beforeDevCommand` and `beforeBuildCommand` in `tauri.conf.json` SHALL invoke the Vite dev server and production build respectively:

```json
{
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:5173",
    "frontendDist": "../frontend/dist"
  }
}
```

### 6.3 UI structure

The app SHALL use a sidebar + main content layout:

- **Sidebar:** Workspace name, navigation links (Dashboard, Search, Documents, Connectors, Embeddings, Registry, Agents, Server, Settings).
- **Main content area:** Renders the selected view.
- **Top bar:** Search input (accessible from any view), workspace switcher.

### 6.4 Styling

The frontend SHALL use a utility-first CSS approach (Tailwind CSS) for styling. The app SHALL support light and dark themes, following the system preference by default with a manual override in settings.

---

## 7. Data flow

### 7.1 Workspace lifecycle

```
User opens app
    → App reads settings (recent workspaces)
    → App shows welcome screen or last workspace

User opens workspace (path)
    → workspace_open command
    → Backend: load_config(path/ctx.toml)
    → Backend: open SqlitePool(config.db.path)
    → Backend: store Config + pool in AppState
    → Backend: query stats (doc count, chunk count, embedding count)
    → Return WorkspaceInfo to frontend
    → Frontend renders dashboard

User closes workspace
    → workspace_close command
    → Backend: close SqlitePool
    → Backend: clear AppState.workspace
    → Frontend returns to welcome screen
```

### 7.2 Sync flow

```
User clicks "Sync" (target = "all" or specific connector)
    → sync_start command
    → Backend: spawn tokio task
    → Backend: return operation_id immediately
    → Task: iterate connectors, call ingest pipeline
    → Task: emit sync-progress events (scanning, processing, complete)
    → Frontend: listen for sync-progress events, update UI in real-time
    → Task completes: emit final sync-progress with phase = "complete"
```

### 7.3 Search flow

```
User types query, selects mode
    → search command (query, mode, limit, filters)
    → Backend: call search_documents(config, query, mode, source, since, limit, explain)
    → Backend: return Vec<SearchResultItem>
    → Frontend: render results list

User clicks a result
    → document_get command (id)
    → Backend: call get_document(config, id)
    → Backend: return DocumentResponse
    → Frontend: render document detail view
```

---

## 8. Platform considerations

### 8.1 Desktop targets (Phase 1)

The app SHALL build for:

| Platform | Webview | Package format |
|----------|---------|----------------|
| macOS (aarch64, x86_64) | WKWebView | `.dmg` |
| Windows (x86_64) | WebView2 | `.msi`, `.exe` (NSIS) |
| Linux (x86_64) | WebKitGTK | `.AppImage`, `.deb` |

### 8.2 Auto-update

The app SHALL use the `tauri-plugin-updater` for automatic updates. Update checks SHALL occur on app launch (if enabled in settings) and be configurable. The update endpoint and signing key SHALL be configured in `tauri.conf.json`.

### 8.3 Mobile (future)

Mobile targets (iOS, Android) are deferred to Phase 3 of the PRD. The architecture supports mobile via Tauri 2.0's mobile capabilities. No mobile-specific code is required in Phase 1, but the crate structure SHALL NOT preclude it (e.g., no desktop-only system calls in the command layer without `#[cfg]` gating).

---

## 9. Security

### 9.1 Tauri capabilities

The app SHALL use Tauri's capability-based permission system. A capability file SHALL be defined in `capabilities/` that grants only the permissions the app needs:

- **Filesystem:** Read and write access scoped to workspace directories and the app data directory. No blanket filesystem access.
- **Shell:** Restricted. No arbitrary command execution. Git operations go through the `context-harness` library (which spawns `git` internally), not through frontend-initiated shell commands.
- **HTTP:** Restricted to the embedding provider endpoints and registry URLs configured in the workspace.
- **Clipboard:** Read/write for copying MCP endpoint URLs and search results.

### 9.2 Credential handling

API tokens (OpenAI, Jira, Slack, etc.) are stored in `ctx.toml` using `${VAR_NAME}` environment variable expansion, as defined in the existing config system. The app SHALL NOT store credentials in its own settings or in plaintext outside of `ctx.toml`. The app MAY provide a UI for setting environment variables or integrating with the OS keychain in a future phase.

---

## 10. Acceptance criteria

The app SHALL be considered complete for Phase 1 (MVP) when all of the following hold:

- **Build:** `cargo tauri build` succeeds for macOS, Windows, and Linux from the workspace root. The produced binaries are functional on each platform.
- **Workspace:** A user can create a new workspace, open an existing workspace, switch between recent workspaces, and view the workspace dashboard with accurate stats.
- **Connectors:** A user can add, edit, remove, and test filesystem and Git connectors through the UI. Connector status is displayed accurately.
- **Sync:** A user can trigger a sync for all connectors or a specific connector. Progress is displayed in real-time. Sync history is viewable.
- **Search:** A user can search in keyword, semantic, and hybrid modes. Results match the CLI and MCP server output for the same query and data. Document preview works for markdown and plain text.
- **Documents:** A user can browse documents by source and view document detail with metadata and chunks.
- **Embeddings:** A user can view embedding status, configure the embedding provider, and trigger embedding for pending chunks.
- **Performance:** App binary is under 15 MB on macOS. App starts in under 2 seconds. Idle memory usage is under 100 MB.
- **Themes:** Light and dark themes work. System theme preference is respected.

---

## 11. Implementation order

Implementation SHALL follow this order. Each step SHALL be verified before proceeding.

1. **Scaffold Tauri app.** Create the `crates/context-harness-app/` directory, initialize Tauri 2.0 with Svelte frontend, configure `tauri.conf.json`, add to workspace members. Verify `cargo tauri dev` launches an empty app window.

2. **App state and workspace commands.** Implement `AppState`, `workspace_open`, `workspace_close`, `workspace_get_info`, `workspace_list_recent`. Set up tauri-specta for binding generation. Verify the frontend can open a workspace and display stats.

3. **Search and document commands.** Implement `search`, `document_get`, `document_list`. Build the search UI and document preview. Verify search results match the CLI.

4. **Connector commands.** Implement `connector_list`, `connector_add`, `connector_update`, `connector_remove`, `connector_test`. Build the connector configuration UI.

5. **Sync commands.** Implement `sync_start`, `sync_cancel`, and the `sync-progress` event system. Build the sync progress UI with real-time updates.

6. **Embedding commands.** Implement `embedding_status`, `embedding_run_pending`, `embedding_rebuild`, `embedding_update_config`. Build the embedding management UI.

7. **Settings and polish.** Implement `settings_get`, `settings_update`. Add theme support, recent workspaces persistence, and the welcome screen. Platform testing across macOS, Windows, and Linux.

8. **Packaging.** Configure Tauri bundler for `.dmg`, `.msi`/`.exe`, `.AppImage`/`.deb`. Set up auto-updater. Verify packages install and run on each platform.

---

## 12. References

- [ADR-0020](../adr/0020-cross-platform-framework-selection.md) -- Framework decision (Tauri 2.0).
- [PRD-0011](../prd/0011-native-app.md) -- Product requirements and phasing.
- [SPEC-0002](0002-workspace-refactor.md) -- Core/app crate split and Store trait.
- [SPEC-0006](0006-json-schemas.md) -- MCP tool response shapes (search, get, sources).
- [SPEC-0011](0011-mcp-agents.md) -- Agent system design and types.
- [DESIGN-0002](../design/0002-sync-progress.md) -- Sync progress reporting behavior.
- [SPEC-0000](0000-spec-policy.md) -- Document classification policy.
- [Tauri 2.0 documentation](https://v2.tauri.app/)
- [tauri-specta](https://github.com/oscartbeaumont/tauri-specta) -- Type-safe IPC bindings.
- [Svelte 5 documentation](https://svelte.dev/docs/svelte)
