# RUNBOOK-0003: Build the Tauri Desktop App

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook builds the Context Harness Tauri desktop application for macOS and installs it to `/Applications`. Use it when building the native app for distribution or local testing.

## Prerequisites

- [RUNBOOK-0001](0001-local-dev-setup.md) completed (Rust, Node.js, repo cloned)
- [RUNBOOK-0002](0002-build-cli.md) — CLI builds successfully
- On macOS in a Nix shell: `LIBRARY_PATH` workaround may be required (see Steps)

## Steps

1. Install frontend dependencies.

   ```bash
   cd crates/context-harness-app/frontend
   npm install
   ```

   Expected output (or similar):

   ```
   added 150 packages in 15s
   ```

2. Return to the app crate root and set `LIBRARY_PATH` before building (macOS in Nix shell).

   ```bash
   cd crates/context-harness-app
   export LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"
   ```

3. Build the Tauri app (release).

   ```bash
   cargo tauri build
   ```

   Expected output (or similar):

   ```
   Running npm run build in ./frontend
   ...
   Compiling context-harness-app v...
   Finished release [optimized] target(s) in ...
   App built successfully. Location: target/release/bundle/macos/
   ```

4. Locate the built app.

   ```bash
   ls -la target/release/bundle/macos/
   ```

   Expected: `Context Harness.app` (or similar) in the output. The `tauri.conf.json` has `"targets": ["app"]`, so no DMG is produced; only the `.app` bundle.

5. Install to `/Applications` (optional).

   ```bash
   cp -R target/release/bundle/macos/*.app /Applications/
   ```

   Expected: `Context Harness.app` appears in `/Applications`.

6. Launch the app.

   ```bash
   open /Applications/Context\ Harness.app
   ```

   Expected: App window opens.

## Development mode

For development with hot-reload of the frontend:

```bash
cd crates/context-harness-app
export LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"
cargo tauri dev
```

Expected: Build runs, frontend dev server starts at `http://localhost:5173`, app window opens with live reload.

## Verification

- `cargo tauri build` completes without errors.
- `target/release/bundle/macos/*.app` exists.
- `open /Applications/Context\ Harness.app` launches the app.
- App window shows the Context Harness UI.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `ld: library not found` or `-lc++` on macOS | System C++ libs not found in Nix shell | Set `LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"` before `cargo tauri build` |
| `npm run build` fails in frontend | Missing deps or Node version | Run `npm install` in `crates/context-harness-app/frontend`; ensure Node 18+ |
| `cargo tauri build` fails with "beforeBuildCommand" | Frontend build failed | Check `npm run build` in `frontend/`; fix any TypeScript or build errors |
| No DMG in output | DMG bundling disabled | `tauri.conf.json` has `"targets": ["app"]`; only `.app` is produced. To enable DMG, change targets in config and rebuild |
| App crashes on launch | Missing runtime deps | Ensure Xcode Command Line Tools installed: `xcode-select --install` |

## Related Runbooks

- [RUNBOOK-0001](0001-local-dev-setup.md) — Local Development Setup
- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI
