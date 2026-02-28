# ADR-0017: rustls Over OpenSSL

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness makes HTTPS requests in several components:

- S3 connector (SigV4-signed API calls)
- Git connector (HTTPS clone/fetch)
- OpenAI embedding provider (API calls)
- Ollama embedding provider (local HTTPS)
- Lua scripted connectors and tools (HTTP host API)
- Extension registry operations (Git clone/pull)

The TLS implementation choice affects build complexity, binary portability,
and the ability to produce fully static binaries for musl targets.

OpenSSL has historically been the default TLS library in the Rust ecosystem,
but it is a C library that requires:

- System headers and libraries at build time
- Shared libraries at runtime (unless statically linked)
- Platform-specific build configuration
- Special handling in Nix builds (pkg-config paths, cross-compilation)

## Decision

Use **rustls** as the sole TLS implementation across the entire dependency
tree. Specifically:

- `reqwest` is configured with the `rustls-tls` feature (not `default-tls`
  or `native-tls`)
- No crate in the dependency tree depends on `openssl-sys`
- The `rmcp` crate (MCP server) uses rustls via its HTTP dependencies

rustls is a pure-Rust TLS implementation that uses `ring` or `aws-lc-rs`
for cryptographic operations. It requires no system libraries, no headers,
and no pkg-config configuration.

## Alternatives Considered

**OpenSSL (via `openssl-sys`).** The most widely deployed TLS library with
comprehensive protocol support and FIPS certification. However:
- Requires `libssl-dev` / `openssl-devel` headers at build time.
- In Nix builds, `pkg-config` must find the correct OpenSSL path, which is
  fragile across nixpkgs versions and cross-compilation targets.
- Static linking OpenSSL for musl targets requires building OpenSSL from
  source with musl-specific configuration, adding significant build
  complexity.
- The decision to use Nix for builds (see [ADR-0016](0016-nix-for-builds.md))
  amplifies the OpenSSL friction, as Nix's hermetic build environment
  makes finding system OpenSSL non-trivial.

**native-tls (via `native-tls` crate).** Uses the platform's native TLS
implementation (SChannel on Windows, Secure Transport on macOS, OpenSSL on
Linux). Provides platform-native certificate handling but has the same
OpenSSL issues on Linux, plus inconsistent behavior across platforms
(different cipher suites, protocol versions, certificate validation logic).

**No TLS (HTTP only).** Not viable — S3 APIs, OpenAI APIs, and most Git
repositories require HTTPS. Local-only connections (Ollama, MCP server
on localhost) could use HTTP, but the connectors and embedding providers
that reach external services cannot.

**Conditional TLS (rustls on musl, OpenSSL elsewhere).** Use rustls only
for problematic targets and OpenSSL on standard Linux/macOS. This would
reduce the risk of rustls-specific incompatibilities but doubles the TLS
testing surface and creates inconsistent behavior across platforms.
Simplicity favors a single implementation everywhere.

## Consequences

- No native library dependencies for TLS on any platform. The binary is
  self-contained, which is critical for the musl static binary and
  single-binary distribution goals.
- Nix builds work without OpenSSL overlays, pkg-config configuration, or
  environment variable overrides. This eliminates a common class of build
  failures.
- rustls does not support all TLS features that OpenSSL does (e.g., certain
  legacy cipher suites, client certificates with PKCS#12). This has not
  been an issue in practice since Context Harness connects to modern
  services (AWS, OpenAI, GitHub) that support rustls's cipher suite.
- Certificate verification uses the `webpki-roots` crate (Mozilla's root
  certificates) rather than the system certificate store. This means
  custom enterprise CA certificates are not automatically trusted. Users
  in corporate environments with TLS inspection may need to configure
  `REQUESTS_CA_BUNDLE` or similar.
- Binary size is slightly smaller than with statically-linked OpenSSL,
  since rustls is compiled as Rust code (benefiting from LTO and dead code
  elimination) rather than linking a pre-compiled C library.
