# ADR: Cross-Platform Framework Selection

**Status:** Accepted  
**Type:** Architecture Decision Record  
**Date:** 2026-02-28  
**Scope:** Framework selection for the Context Harness native desktop and mobile app.

---

## 1. Context

Context Harness is a Rust-based local-first context ingestion and retrieval framework. It currently ships as a library crate (`context_harness`) and a CLI binary (`ctx`). The planned workspace refactor (see [SPEC-0002](../spec/0002-workspace-refactor.md)) will split the codebase into a WASM-safe `context-harness-core` and a native `context-harness` crate. This creates the foundation for a standalone native app that depends on the core library.

The native app is a separate product from the CLI. It provides a visual interface for workspace management, connector configuration, sync, search, document browsing, and extension management. The target audience starts with individual developers and expands to teams.

This ADR records the framework evaluation and decision for building that app.

---

## 2. Decision drivers

The following criteria were used to evaluate candidate frameworks. Each is weighted to reflect its importance to this project.

| # | Criterion | Weight | Rationale |
|---|-----------|--------|-----------|
| 1 | Language alignment | High (x3) | The core library is Rust. Direct crate integration avoids FFI overhead, serialization boundaries, and maintenance cost. |
| 2 | Binary size and resource usage | High (x3) | "Local-first" implies lightweight, fast-starting, low memory. Users run this alongside IDEs and other tools. |
| 3 | Cross-platform coverage | High (x3) | Must support macOS, Windows, and Linux desktops. Mobile (iOS, Android) is a future goal. |
| 4 | Security model | Medium (x2) | The app handles local files, credentials (API tokens for connectors), and potentially sensitive knowledge bases. |
| 5 | Frontend flexibility | Medium (x2) | Ability to use modern web UI patterns (component frameworks, CSS, design systems). |
| 6 | Ecosystem and maturity | Medium (x2) | Plugin ecosystem, community size, production track record, and long-term viability. |
| 7 | Developer experience | Medium (x2) | Hot reload, debugging, tooling, onboarding effort for contributors. |
| 8 | Maintenance burden | Low (x1) | Number of separate codebases, platform-specific code, and CI complexity. |

---

## 3. Options considered

### 3.1 Tauri 2.0

**Architecture:** Rust backend process communicates with a frontend rendered in the OS-native webview (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux) via JSON IPC. The frontend is any web technology (HTML/CSS/JS). Tauri 2.0 adds iOS and Android support.

**Key characteristics:**
- Backend is Rust -- `context-harness-core` is a direct `[dependency]` in `Cargo.toml`.
- Minimal binary: ~3-5 MB for a simple app. No bundled browser engine.
- Memory: ~40-50 MB idle.
- Security: permission-based capability model; frontend is sandboxed and cannot access system resources without explicit Rust-side grants.
- Frontend: any framework that compiles to HTML/JS/CSS (React, Svelte, SolidJS, Vue, etc.).
- Mobile: iOS and Android targets available in Tauri 2.0.
- Type-safe IPC via tauri-specta or taurpc (auto-generated TypeScript bindings from Rust types).
- Updater plugin for auto-updates; bundler produces `.dmg`, `.msi`, `.AppImage`, `.deb`.

### 3.2 Electron

**Architecture:** Bundles Chromium and Node.js. The main process (Node.js) communicates with renderer processes (Chromium) via IPC.

**Key characteristics:**
- Backend is Node.js -- Rust integration requires building a native addon via napi-rs or N-API, or spawning a sidecar process and communicating over IPC/HTTP.
- Binary: 80-150 MB minimum (bundled Chromium).
- Memory: 150-400+ MB idle.
- Security: historically permissive; `nodeIntegration` and `contextIsolation` require careful configuration.
- Frontend: full Chromium, so any web framework works with perfect consistency.
- Desktop only (macOS, Windows, Linux). No mobile.
- Mature ecosystem: VS Code, Slack, Discord, Figma (migrated away) are built on Electron.

### 3.3 Flutter

**Architecture:** Dart framework with Skia/Impeller rendering engine. Compiles to native code. Does not use a webview or browser engine -- renders its own pixel-perfect UI.

**Key characteristics:**
- Backend is Dart -- Rust integration requires FFI via `dart:ffi` and a C ABI boundary (`cbindgen` or `flutter_rust_bridge`).
- Binary: 15-25 MB (desktop), 8-15 MB (mobile).
- Memory: 80-120 MB idle (desktop).
- Security: standard OS sandbox; no built-in capability model.
- Frontend: Flutter's own widget system (Material, Cupertino). Not web technology; cannot use React/Svelte/etc.
- Cross-platform: desktop (macOS, Windows, Linux) and mobile (iOS, Android).
- Hot reload in debug mode.
- Requires learning Dart and Flutter's widget paradigm.

### 3.4 Platform-native

**Architecture:** Separate implementations per platform using native toolkits: SwiftUI (macOS/iOS), Kotlin + Jetpack Compose (Android), WinUI 3 / .NET MAUI (Windows).

**Key characteristics:**
- Rust integration via C FFI on each platform (Swift/Kotlin/C# interop with Rust via `cbindgen` or `uniffi`).
- Binary: smallest possible per platform (native code, no runtime).
- Memory: lowest possible per platform.
- Security: full OS-native sandbox and permission model.
- Frontend: native look-and-feel, platform-specific UI paradigms.
- Three separate frontend codebases (at minimum). No code sharing for UI.
- Highest ongoing maintenance cost.

### 3.5 React Native

**Architecture:** JavaScript runtime with native component rendering (not webview). Uses a bridge (or the new architecture's JSI) to communicate between JS and native modules.

**Key characteristics:**
- Backend is JavaScript -- Rust integration via native modules (turbo modules + FFI) or HTTP sidecar.
- Binary: 15-30 MB (mobile), desktop support via react-native-windows and react-native-macos (less mature).
- Memory: 80-150 MB.
- Security: standard OS sandbox.
- Frontend: React component model with native primitives.
- Primary strength is mobile (iOS, Android). Desktop support exists but is secondary.
- Large ecosystem for mobile; desktop ecosystem is thin.

---

## 4. Evaluation matrix

Each option is scored 1-5 per criterion (5 = best). The weighted score is `score x weight`.

| Criterion | Weight | Tauri 2.0 | Electron | Flutter | Platform-native | React Native |
|-----------|--------|-----------|----------|---------|-----------------|--------------|
| Language alignment | x3 | **5** (direct Rust crate) | 2 (FFI/sidecar) | 3 (FFI via bridge) | 3 (FFI per platform) | 2 (FFI/sidecar) |
| Binary size / resources | x3 | **5** (3-5 MB, ~40 MB RAM) | 1 (80-150 MB, 150+ MB RAM) | 4 (15-25 MB, ~100 MB RAM) | **5** (minimal) | 3 (15-30 MB, ~100 MB RAM) |
| Cross-platform coverage | x3 | **5** (desktop + mobile) | 3 (desktop only) | **5** (desktop + mobile) | 4 (all, but separate codebases) | 4 (mobile strong, desktop weak) |
| Security model | x2 | **5** (capability-based permissions) | 2 (manual hardening) | 3 (OS sandbox) | **5** (OS-native) | 3 (OS sandbox) |
| Frontend flexibility | x2 | **5** (any web framework) | **5** (full Chromium) | 2 (Dart widgets only) | 2 (platform-native only) | 4 (React paradigm) |
| Ecosystem / maturity | x2 | 3 (growing, 2.0 stable Oct 2024) | **5** (decade of production use) | 4 (mature, large community) | 4 (platform SDKs are mature) | 4 (mature for mobile) |
| Developer experience | x2 | 4 (hot reload for frontend, Rust for backend) | **5** (Chrome DevTools, hot reload) | 4 (hot reload, Dart tooling) | 3 (per-platform tooling) | 4 (hot reload, React DevTools) |
| Maintenance burden | x1 | 4 (single codebase) | 4 (single codebase) | 3 (single codebase, Dart + Rust FFI) | 1 (3+ codebases) | 3 (JS + native modules per platform) |

### Weighted totals

| Framework | Weighted score |
|-----------|---------------|
| **Tauri 2.0** | **85** |
| Flutter | 67 |
| Platform-native | 63 |
| Electron | 59 |
| React Native | 58 |

---

## 5. Analysis

### 5.1 Why Tauri wins

**Language alignment is the decisive factor.** Context Harness is a Rust library. Tauri's backend is Rust. The app crate adds `context-harness-core` (and `context-harness`) as workspace dependencies in `Cargo.toml` and calls library functions directly -- no FFI layer, no serialization boundary for internal calls, no separate process. Every other framework requires bridging Rust to a foreign runtime (Node.js, Dart, Swift, Kotlin, C#), adding complexity, maintenance cost, and potential for impedance mismatch.

**Resource efficiency aligns with product philosophy.** Context Harness is "local-first" -- it runs alongside IDEs, editors, and other developer tools. A 3-5 MB binary using 40 MB of RAM fits that story. An Electron app consuming 150+ MB of RAM before doing any work does not.

**Cross-platform coverage matches the roadmap.** Tauri 2.0 supports macOS, Windows, Linux, iOS, and Android from a single codebase. The desktop targets are mature; mobile is newer but functional and improving.

**Security model is a natural fit.** The capability-based permission system means the frontend webview cannot access the filesystem, spawn processes, or make network requests without explicit Rust-side grants. This matters for an app that manages local knowledge bases and stores API credentials.

### 5.2 Risks and mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Webview rendering inconsistency across platforms | Medium | Test on all three desktop webviews in CI. Avoid bleeding-edge CSS features. Use a design system with cross-browser compatibility. |
| Tauri ecosystem is younger than Electron's | Low | Core functionality (IPC, bundling, auto-update) is stable. The app's complexity is in the Rust backend, not in Tauri plugins. |
| Mobile targets (iOS/Android) are less mature | Low | Desktop is the Phase 1 target. Mobile is Phase 2+, by which time Tauri mobile will have further matured. |
| Dual-language stack (Rust + TypeScript) | Low | The team already writes Rust. TypeScript frontend is standard web development. Type-safe IPC (tauri-specta) bridges the gap. |

### 5.3 Why not the runners-up

**Flutter** scores well on cross-platform and resources, but the FFI bridge to Rust (via `flutter_rust_bridge` or raw `dart:ffi`) adds a maintenance layer that Tauri eliminates entirely. Flutter's widget system also locks out the web frontend ecosystem -- no React, Svelte, or existing design systems built for the web.

**Platform-native** delivers the best per-platform experience but at 3x the frontend maintenance cost. With a small team, this is not viable for Phase 1.

**Electron** has the most mature ecosystem, but the resource overhead conflicts with the "local-first, lightweight" positioning, and the Node.js backend requires an FFI bridge to Rust.

---

## 6. Decision

**Tauri 2.0** is selected as the framework for the Context Harness native app.

The app will be structured as a new crate in the Cargo workspace (after the workspace refactor), with the Tauri Rust backend depending directly on `context-harness-core` and `context-harness`. The frontend will use a lightweight web framework (to be decided in [SPEC-0001](../spec/0001-native-app.md)) rendered in the system webview.

---

## 7. Consequences

- The workspace refactor ([SPEC-0002](../spec/0002-workspace-refactor.md)) is a prerequisite. The app crate depends on `context-harness-core` existing as a separate, importable crate.
- Frontend development uses web technologies (HTML/CSS/JS/TS). Contributors need basic web development familiarity in addition to Rust.
- Platform testing must cover all three desktop webviews (WebView2, WKWebView, WebKitGTK). CI should include macOS, Windows, and Linux runners.
- Mobile support (iOS, Android) is deferred but architecturally possible without a framework change.

---

## 8. References

- [SPEC-0002](../spec/0002-workspace-refactor.md) -- Cargo workspace split and Store abstraction.
- [SPEC-0000](../spec/0000-spec-policy.md) -- Document classification (this is a design/decision doc, not a spec).
- [Tauri 2.0 documentation](https://v2.tauri.app/)
- [tauri-specta](https://github.com/oscartbeaumont/tauri-specta) -- Type-safe IPC bindings.
