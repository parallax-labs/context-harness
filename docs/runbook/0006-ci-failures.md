# RUNBOOK-0006: Diagnose CI Failures

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook helps diagnose and fix CI failures in the Context Harness pipeline. The CI workflow (`.github/workflows/ci.yml`) runs on push/PR to `main` and includes formatting checks, Clippy, tests, and a 6-target build matrix. Use this runbook when a CI run fails and you need to reproduce the failure locally and apply a fix.

## Prerequisites

- Local development environment set up (see [RUNBOOK-0001](0001-local-dev-setup.md))
- Rust stable toolchain with `rustfmt` and `clippy` components
- For Zig targets: Zig installed and `cargo-zigbuild` available

## Steps

### 1. Identify the failing job

Open the failed workflow run on GitHub Actions. CI has two job types:

- **Check & Test** — formatting, Clippy, tests (runs on `ubuntu-latest`)
- **Build** — 6-target matrix (linux-x86_64, linux-x86_64-musl, linux-aarch64, macos-x86_64, macos-aarch64, windows-x86_64)

Note which job failed and the exact error message.

### 2. Reproduce locally

Run the equivalent commands locally. The sections below map each failure type to reproduction steps and common fixes.

---

## Failure type: Formatting (`cargo fmt`)

**CI step:** `Check formatting` → `cargo fmt --all -- --check`

**Reproduce locally:**

```bash
cargo fmt --all -- --check
```

**Expected (failure):** Exit code 1; diff of files that need formatting.

**Fix:**

```bash
cargo fmt --all
git add -u
git commit -m "style: apply rustfmt"
```

---

## Failure type: Clippy warnings

**CI step:** `Clippy` → `cargo clippy --workspace --all-targets --all-features -- -D warnings`

**Reproduce locally:**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

**Expected (failure):** Clippy reports warnings or errors; exit code 1.

**Fix:** Address each Clippy suggestion. Common patterns:

- Add `#[allow(clippy::...)]` for intentional patterns (use sparingly)
- Fix the underlying issue (e.g. unnecessary clones, missing `Default`, etc.)
- Run `cargo clippy --fix` for auto-fixable lints (review changes before committing)

---

## Failure type: Test failures

**CI step:** `Run tests` → `cargo test --workspace --all-features`

**Reproduce locally:**

```bash
cargo test --workspace --all-features
```

**Expected (failure):** One or more tests fail; backtrace or assertion message shown.

**Fix:** Inspect the failing test output. Common causes:

- Flaky test (timing, filesystem, network) — add retries or stabilize the test
- Environment difference (paths, env vars) — use `tempfile` or mock external deps
- Logic bug — fix the code or update the test expectation

---

## Failure type: Cross-compile failures (Zig targets)

**CI step:** `Build linux-x86_64-musl` (the only Zig target in the matrix)

**Target:** `x86_64-unknown-linux-musl` with `--no-default-features --features local-embeddings-tract`

**Reproduce locally:**

```bash
# Install Zig and cargo-zigbuild if not already present
cargo install cargo-zigbuild
# Or: zig --version  # ensure Zig is in PATH

cargo zigbuild --release --target x86_64-unknown-linux-musl \
  -p context-harness --no-default-features --features local-embeddings-tract
```

**Expected (failure):** Linker errors, missing C/C++ toolchain, or crate compilation errors.

**Common fixes:**

| Error | Cause | Fix |
|-------|-------|-----|
| `cargo-zigbuild: command not found` | cargo-zigbuild not installed | `cargo install cargo-zigbuild` |
| `zig: command not found` | Zig not installed or not in PATH | Install Zig; add to PATH |
| Linker errors (e.g. `undefined reference`) | Zig musl toolchain or C++ libs | Ensure Zig provides musl; check that `tract` (pure Rust) is used, not `fastembed` (C++/ORT) |
| Crate fails to compile for musl | Incompatible code or feature | Verify `local-embeddings-tract` is used for musl; avoid platform-specific code without `#[cfg]` |

---

## Failure type: Build matrix (non-Zig targets)

**CI step:** Any of `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`

**Reproduce locally:** Use the same target and features as the failing job.

```bash
# Example: linux-x86_64 (default features)
cargo build --release --target x86_64-unknown-linux-gnu -p context-harness

# Example: macos-x86_64 (tract features)
cargo build --release --target x86_64-apple-darwin -p context-harness \
  --no-default-features --features local-embeddings-tract

# Example: windows-x86_64
cargo build --release --target x86_64-pc-windows-msvc -p context-harness
```

**Note:** You may not have all targets installed. Add them with:

```bash
rustup target add x86_64-unknown-linux-gnu
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin
rustup target add x86_64-pc-windows-msvc
```

**Common fixes:**

| Error | Cause | Fix |
|-------|-------|-----|
| `linker 'cc' not found` | Cross-compilation toolchain missing | Install the appropriate toolchain (e.g. `mingw-w64` for Windows from macOS/Linux) |
| `fastembed` / ORT build fails | C++ runtime or OpenSSL issues | Use `--no-default-features --features local-embeddings-tract` for problematic targets |
| macOS `-lc++` or SDK errors | Nix or system SDK path | Set `LIBRARY_PATH` (see [RUNBOOK-0001](0001-local-dev-setup.md)) |
| aarch64 Linux fails locally | No ARM runner | CI uses `ubuntu-24.04-arm`; rely on CI for that target or use QEMU/remote ARM |

---

## Verification

After applying fixes:

1. Run the full check suite locally:

   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   ```

2. If you have Zig and the musl target:

   ```bash
   cargo zigbuild --release --target x86_64-unknown-linux-musl \
     -p context-harness --no-default-features --features local-embeddings-tract
   ```

3. Push your changes and confirm CI passes.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| CI passes locally but fails on GitHub | Caching, environment, or runner differences | Clear Actions cache for the workflow; compare `rustc --version` and `cargo --version` |
| Only one matrix job fails | Platform-specific bug or dependency | Reproduce with that exact target and features; check for `#[cfg]` or feature-gated code paths |
| `Check & Test` passes but `Build` fails | Build uses different features or targets | CI build uses `-p context-harness` with per-target features; ensure you test the same combination |
| Intermittent test failure | Flaky test | Add retries, increase timeouts, or mock non-deterministic behavior |
| Clippy suggests changes that conflict with project style | Project-specific lint config | Check for `clippy.toml` or `#![allow(...)]` in the crate; follow project conventions |

## Related Runbooks

- [RUNBOOK-0001](0001-local-dev-setup.md) — Local Development Setup
- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI
- [RUNBOOK-0005](0005-cut-release.md) — Cut a Release (uses same build matrix as release workflow)
