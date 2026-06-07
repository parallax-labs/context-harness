# RUNBOOK-0002: Build the CLI

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook builds the Context Harness CLI binary (`ctx`) for development (debug) or production (release). Use it when iterating on CLI changes, preparing a release, or installing the tool locally.

## Prerequisites

- [RUNBOOK-0001](0001-local-dev-setup.md) completed (Rust toolchain, repo cloned)
- On macOS in a Nix shell: `LIBRARY_PATH` workaround may be required (see Steps)

## Steps

1. (macOS in Nix shell only) If you previously saw linker errors, set `LIBRARY_PATH` before building:

   ```bash
   export LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"
   ```

2. Build the CLI in debug mode (fast compile, larger binary, slower runtime).

   ```bash
   cargo build -p context-harness
   ```

   Expected output (or similar):

   ```
   Compiling context-harness-core v...
   Compiling context-harness v...
   Finished dev [unoptimized + debuginfo] target(s) in ...
   ```

   Binary location: `target/debug/ctx`

3. Build the CLI in release mode (optimized, smaller binary, faster runtime).

   ```bash
   cargo build --release -p context-harness
   ```

   Expected output (or similar):

   ```
   Compiling ...
   Finished release [optimized] target(s) in ...
   ```

   Binary location: `target/release/ctx`

4. (Optional) Install the CLI to your `~/.cargo/bin` for global use.

   ```bash
   cargo install --path crates/context-harness
   ```

   Expected output (or similar):

   ```
   Compiling context-harness v...
   Installing ~/.cargo/bin/ctx
   ```

5. Verify the installation.

   ```bash
   ctx --help
   ```

   Expected output (or similar):

   ```
   A local-first context ingestion and retrieval framework for AI tools

   Usage: ctx [OPTIONS] <COMMAND>

   Commands:
     serve    Start the MCP server
     ...
   ```

## Feature flags

The default build uses **fastembed** with ONNX Runtime (ORT) for embeddings. This requires C++ toolchain support and downloads ORT binaries at build time.

For a **pure-Rust** build without C++ dependencies (e.g. musl Linux, Intel Mac, or constrained environments), use the tract backend:

```bash
cargo build -p context-harness --no-default-features --features local-embeddings-tract
```

Expected: Build completes; no ORT download; embeddings use `tract-onnx` and `tokenizers`.

## Verification

- `target/debug/ctx --help` or `target/release/ctx --help` prints usage.
- If installed: `which ctx` shows `~/.cargo/bin/ctx` (or equivalent).
- `ctx --version` prints a version string.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `ld: library not found` or `-lc++` on macOS | System C++ libs not found in Nix shell | Set `LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"` before `cargo build` |
| `ort-sys` or `fastembed` build fails | ORT download failed; network or sandbox issue | Ensure network access; if using Nix: `nix build --option sandbox false .#with-embeddings` or use `cargo build` in `nix develop` |
| `openssl-sys` fails on Linux | OpenSSL dev headers missing | Install `libssl-dev` (Debian/Ubuntu) or use `nix develop` |
| `ctx: command not found` after install | `~/.cargo/bin` not in PATH | Add `export PATH="$HOME/.cargo/bin:$PATH"` to `~/.bashrc` or `~/.zshrc` |

## Related Runbooks

- [RUNBOOK-0001](0001-local-dev-setup.md) — Local Development Setup
- [RUNBOOK-0004](0004-run-tests.md) — Run Tests and Checks
