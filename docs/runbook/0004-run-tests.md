# RUNBOOK-0004: Run Tests and Checks

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook runs the full test suite and code-quality checks for Context Harness. Use it before committing, pushing, or opening a PR; CI runs the same commands.

## Prerequisites

- [RUNBOOK-0001](0001-local-dev-setup.md) completed (Rust toolchain, repo cloned)
- On macOS in a Nix shell: `LIBRARY_PATH` workaround may be required (see Steps)

## Steps

1. (macOS in Nix shell only) Set `LIBRARY_PATH` before running tests if you previously saw linker errors:

   ```bash
   export LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"
   ```

2. Run all workspace tests with all features.

   ```bash
   cargo test --workspace --all-features
   ```

   Expected output (or similar):

   ```
   running 45 tests
   test context_harness_engine::tests::test_foo ... ok
   ...
   test result: ok. 45 passed; 0 failed; 0 ignored
   ```

3. Run format check (no changes; fails if code is not formatted).

   ```bash
   cargo fmt --all -- --check
   ```

   Expected output: no output (success). If formatting is needed:

   ```
   Diff in ...
   ```

   Fix with: `cargo fmt --all`

4. Run Clippy (linter) with warnings as errors.

   ```bash
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   ```

   Expected output (or similar):

   ```
   Checking context-harness-core v...
   Checking context-harness v...
   Checking context-harness-app v...
   Finished dev [unoptimized + debuginfo] target(s) in ...
   ```

5. (Optional) Run tests for a single crate only.

   ```bash
   cargo test -p context-harness --all-features
   cargo test -p context-harness-core --all-features
   cargo test -p context-harness-app --all-features
   ```

   Expected: Same pattern as step 2, but only for the specified crate.

## Verification

- `cargo test --workspace --all-features` exits with code 0.
- `cargo fmt --all -- --check` exits with code 0.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` exits with code 0.
- CI (GitHub Actions or equivalent) runs these same commands and passes.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `ld: library not found` or `-lc++` on macOS | System C++ libs not found in Nix shell | Set `LIBRARY_PATH="/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib:${LIBRARY_PATH:-}"` before `cargo test` |
| Integration test fails with `git` | Git not installed or not in PATH | Install Git; Nix shell provides it via `nativeCheckInputs` |
| `cargo fmt --all -- --check` fails | Code not formatted | Run `cargo fmt --all` and commit |
| Clippy reports warnings | Lint violations | Fix the reported issues; `-D warnings` treats warnings as errors |
| Test fails with "database locked" or similar | Concurrent test or stale DB | Run tests in isolation; ensure no other `ctx` process is using the DB |
| `ort-sys` or `fastembed` build fails during test | ORT download or network issue | Ensure network access; use `LIBRARY_PATH` on macOS if needed |

## Related Runbooks

- [RUNBOOK-0001](0001-local-dev-setup.md) — Local Development Setup
- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI
