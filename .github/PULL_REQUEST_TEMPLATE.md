<!--
Branch from main as feature/<short-name> (see CONTRIBUTING.md).
Replace each comment, then delete it.
-->

## Summary

<!-- What changed, and why. Name the spec, ADR, or PRD when this changes behavior or records a decision. -->

## Linked issue

<!-- Fixes #123 — or "None". -->

## Type of change

- [ ] Bug fix
- [ ] Feature
- [ ] Connector, Lua, or Rust extension
- [ ] Documentation or spec
- [ ] Refactor (no behavior change)
- [ ] CI, build, or release

## Checklist

The Check & Test job in [`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs the commands below. Run them before requesting review.

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo clippy -p context-harness --all-targets --no-default-features --features local-embeddings-tract,zvec-bundled -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo test -p context-harness --no-default-features --features local-embeddings-tract,zvec-bundled`

CI also builds the six release targets in the `build` job. Reproduce that matrix locally only when the change affects cross-compilation.

### Docs and changelog

Specs are the behavior contract ([ADR-0015](docs/adr/0015-spec-driven-development.md)). ADRs record architectural decisions ([ADR-0000](docs/adr/0000-adr-policy.md)). Published guides live under `site/content/`. `CHANGELOG.md` follows Keep a Changelog; user-visible changes go under `## [Unreleased]`.

- [ ] `docs/spec/` updated, or this change does not change behavior
- [ ] `docs/adr/` added, or this change does not record an architectural decision
- [ ] `docs/prd/`, `docs/design/`, or `docs/runbook/` updated, or not applicable
- [ ] `site/content/` updated, or the published docs do not change
- [ ] `CHANGELOG.md` updated under `## [Unreleased]`, or the change is not user-visible

## Testing notes

<!--
Commands you ran, and the feature set when it matters
(local-embeddings-tract, zvec-bundled).
Note anything CI does not cover, or any command you could not run.
-->
