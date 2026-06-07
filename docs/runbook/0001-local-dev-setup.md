# RUNBOOK-0001: Local Development Setup

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook gets a new contributor from zero to a working development environment for Context Harness. Use it when setting up a new machine, onboarding a team member, or after a clean OS install.

## Prerequisites

- Git
- A terminal (macOS Terminal, iTerm2, or equivalent)
- (Optional but recommended) [Nix](https://nixos.org/download.html) with flakes enabled

## Steps

1. Clone the repository.

   ```bash
   git clone https://github.com/parallax-labs/context-harness.git
   cd context-harness
   ```

   Expected: Repository contents in the current directory.

2. Install the Rust toolchain (stable). If you use [rustup](https://rustup.rs/):

   ```bash
   rustup default stable
   ```

   Expected output (or similar):

   ```
   info: syncing channel update for 'stable-...'
   info: default toolchain set to 'stable-...'
   ```

3. Install Node.js (required for the Tauri frontend). Use [nvm](https://github.com/nvm-sh/nvm), [fnm](https://github.com/Schniz/fnm), or your system package manager. LTS (20.x or 22.x) is recommended.

   ```bash
   node --version
   npm --version
   ```

   Expected: Version strings (e.g. `v20.10.0`, `10.2.3`).

4. (Optional but recommended) Enter the Nix development shell. This provides a consistent Rust, Clippy, and OpenSSL environment.

   ```bash
   nix develop
   ```

   Expected: Shell prompt changes; `cargo`, `rustc`, `rustfmt`, and `clippy` are available.

5. Perform a first build to verify the setup.

   ```bash
   cargo build -p context-harness
   ```

   On macOS in a Nix shell, if you see linker errors involving `-lc++` or missing system libraries, set `LIBRARY_PATH` before building:

   ```bash
   export LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"
   cargo build -p context-harness
   ```

   Expected output (or similar):

   ```
   Compiling context-harness-core v...
   Compiling context-harness v...
   Finished dev [unoptimized + debuginfo] target(s) in ...
   ```

6. Verify the CLI binary runs.

   ```bash
   target/debug/ctx --help
   ```

   Expected output (or similar):

   ```
   A local-first context ingestion and retrieval framework for AI tools

   Usage: ctx [OPTIONS] <COMMAND>
   ...
   ```

## Verification

- `cargo build -p context-harness` completes without errors.
- `target/debug/ctx --help` prints usage information.
- (If building the Tauri app) `cd crates/context-harness-app && npm install` in `frontend/` and `cargo tauri build` succeed. See [RUNBOOK-0003](0003-build-tauri-app.md).

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `cargo: command not found` | Rust not installed or not in PATH | Run `rustup default stable` or install via [rustup.rs](https://rustup.rs/) |
| `nix: command not found` | Nix not installed | Install from [nixos.org/download](https://nixos.org/download.html); enable flakes with `experimental-features = nix-command flakes` in `~/.config/nix/nix.conf` |
| Linker error: `-lc++` or `ld: library not found` on macOS | Nix shell uses Zig as linker; system libs not found | Set `LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"` before `cargo build` |
| `openssl-sys` build fails on Linux | OpenSSL dev headers missing | Install `libssl-dev` (Debian/Ubuntu) or `openssl-devel` (Fedora/RHEL); Nix shell provides this automatically |
| `node` or `npm` not found | Node.js not installed | Install via nvm, fnm, or system package manager; ensure `node` and `npm` are in PATH |

## Related Runbooks

- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI (debug and release)
- [RUNBOOK-0003](0003-build-tauri-app.md) — Build the Tauri Desktop App
- [RUNBOOK-0004](0004-run-tests.md) — Run Tests and Checks
