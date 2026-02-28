# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for Context Harness.
ADRs capture the key architectural decisions made during the project's development,
including the context, alternatives considered, and consequences of each decision.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-rust-as-implementation-language.md) | Rust as Implementation Language | Accepted |
| [0002](0002-sqlite-as-embedded-storage.md) | SQLite as Embedded Storage | Accepted |
| [0003](0003-fts5-for-keyword-search.md) | FTS5 for Keyword Search | Accepted |
| [0004](0004-brute-force-vector-search.md) | Brute-Force Vector Search with BLOB Storage | Accepted |
| [0005](0005-hybrid-scoring-with-min-max-normalization.md) | Hybrid Scoring with Min-Max Normalization | Accepted |
| [0006](0006-paragraph-boundary-chunking.md) | Paragraph-Boundary Chunking | Accepted |
| [0007](0007-trait-based-extension-system.md) | Trait-Based Extension System | Accepted |
| [0008](0008-lua-for-runtime-extensibility.md) | Lua 5.4 for Runtime Extensibility | Accepted |
| [0009](0009-mcp-streamable-http-transport.md) | MCP Streamable HTTP Transport | Accepted |
| [0010](0010-toml-configuration-with-env-expansion.md) | TOML Configuration with Env Expansion | Accepted |
| [0011](0011-local-first-embedding-providers.md) | Local-First Embedding Providers | Accepted |
| [0012](0012-pure-rust-s3-client.md) | Pure Rust S3 Client | Accepted |
| [0013](0013-git-backed-extension-registries.md) | Git-Backed Extension Registries | Accepted |
| [0014](0014-stateless-agent-architecture.md) | Stateless Agent Architecture | Accepted |
| [0015](0015-spec-driven-development.md) | Spec-Driven Development | Accepted |
| [0016](0016-nix-for-builds.md) | Nix for Builds and Development | Accepted |
| [0017](0017-rustls-over-openssl.md) | rustls Over OpenSSL | Accepted |
| [0018](0018-store-abstraction-and-workspace-split.md) | Store Abstraction and Workspace Split | Accepted |
| [0019](0019-agpl3-license.md) | AGPL-3.0 License | Accepted |

## Creating a New ADR

1. Copy the template below into a new file named `NNNN-short-title.md` where
   `NNNN` is the next sequential number.
2. Fill in all sections. Use concrete details — formats, config keys, module
   names — not vague descriptions.
3. Add an entry to the index table above.
4. Commit alongside the implementation (or retroactively if backfilling).

### Template

```markdown
# ADR-NNNN: Title

**Status:** Proposed | Accepted | Superseded by [ADR-NNNN](NNNN-title.md) | Deprecated
**Date:** YYYY-MM-DD

## Context

Why this decision was needed. What problem or constraint prompted it.

## Decision

What was decided. Be specific — name the technology, pattern, or approach.

## Alternatives Considered

What else was evaluated and why it was rejected.

## Consequences

What follows from this decision — both positive tradeoffs and accepted downsides.
```

## Relationship to Specs

ADRs record *why* a decision was made. Specs (in `docs/`) define *what* the
system does as an authoritative contract. See [SPEC_POLICY.md](../SPEC_POLICY.md)
for the distinction between specs, design docs, and planning docs.

An ADR may reference one or more specs. When a spec changes in a way that
reverses an ADR, the ADR should be marked **Superseded** with a link to the
new ADR that captures the revised decision.
