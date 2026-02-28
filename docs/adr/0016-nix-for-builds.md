# ADR-0016: Nix for Builds and Development

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness targets six platform/architecture combinations:

| Target | Notes |
|--------|-------|
| Linux x86_64 (glibc) | Standard Linux desktops and CI |
| Linux x86_64 (musl) | Static binaries, Alpine, containers |
| Linux aarch64 (glibc) | ARM servers, Raspberry Pi, Graviton |
| macOS x86_64 | Intel Macs |
| macOS aarch64 | Apple Silicon Macs |
| Windows x86_64 | Windows desktops |

The build environment must:

- Provide consistent, reproducible builds across developer machines and CI
- Supply system dependencies (SQLite headers, Lua, etc.) without manual
  installation
- Support cross-compilation for the musl target from glibc hosts
- Manage the Rust toolchain and build tools

## Decision

Use **Nix** with a `flake.nix` for both development shells and builds.

### Development Shell

`nix develop` provides a reproducible environment with:

- Rust toolchain (via `rust-overlay` or `fenix`)
- System dependencies (pkg-config, SQLite dev headers, etc.)
- Build tools (cargo-zigbuild, zig for musl cross-compilation)
- Formatting and linting tools (rustfmt, clippy)

### Cross-Compilation

The **musl** target uses `cargo-zigbuild` with a Zig-provided sysroot.
This avoids the complexity of managing a musl cross-compilation toolchain
directly. Zig provides a complete C toolchain that can target musl from
any host, and `cargo-zigbuild` integrates it transparently with Cargo.

### CI Integration

The GitHub Actions CI workflow uses Nix for reproducible builds across the
matrix. Each target in the build matrix uses the same `flake.nix`-defined
environment, ensuring that CI builds match local developer builds.

The six-target build matrix in CI:

```
linux-x86_64         cargo build --release
linux-x86_64-musl    cargo zigbuild --release --target x86_64-unknown-linux-musl
linux-aarch64        cross-compilation via cargo-zigbuild
macos-x86_64         cargo build --release (cross from Apple Silicon)
macos-aarch64        cargo build --release (native Apple Silicon)
windows-x86_64       cargo build --release
```

## Alternatives Considered

**Docker-only builds.** Docker containers provide reproducible Linux builds
but have significant friction on macOS: Docker Desktop is slow, file system
mounts are slow, and building macOS targets inside Docker is not supported.
Docker is better suited as a deployment artifact than a build environment.

**Makefile / shell scripts.** Simple and widely understood, but not
reproducible. Different developers have different system packages, tool
versions, and configurations. "Works on my machine" problems are common.

**Bare Cargo (no environment management).** `cargo build` works on its own
for pure-Rust code, but Context Harness depends on system libraries (SQLite
headers for sqlx, C compiler for mlua's vendored Lua). Without environment
management, new contributors must manually install these dependencies, with
platform-specific instructions.

**asdf / mise.** Version managers that handle Rust, Node, Python, etc.
Lighter than Nix but less comprehensive — they manage tool versions but not
system libraries. Would still require manual installation of pkg-config,
SQLite headers, and a C compiler.

**Homebrew / apt (platform-specific).** Document required packages for each
platform and have developers install them. Works but is not reproducible
across versions, requires separate instructions per platform, and breaks
when system packages are updated.

## Consequences

- `nix develop` gives any developer a complete, working build environment
  in one command, regardless of their host OS (Linux or macOS).
- Builds are reproducible — the same `flake.lock` produces the same
  environment everywhere, eliminating "works on my machine" issues.
- Nix has a steep learning curve. Contributors unfamiliar with Nix can
  still use `cargo build` directly if they install dependencies manually,
  but the Nix path is the supported and documented approach.
- `cargo-zigbuild` for musl cross-compilation avoids the complexity of
  managing a musl toolchain (musl-gcc, musl headers) directly. Zig bundles
  everything needed.
- The Nix flake pins all dependencies, including the Rust toolchain version.
  Updating the toolchain is a deliberate action (updating `flake.lock`)
  rather than an accidental side effect of system updates.
- Windows builds do not use Nix (Nix support on Windows is limited) and
  rely on the standard Rust toolchain installed via rustup in CI.
