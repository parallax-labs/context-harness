# PRD-0010: Distribution and Packaging

**Status:** Planned
**Date:** 2026-02-27
**Author:** pjones

## Problem Statement

Context Harness is installable today via `cargo install --git` or by
building from source with Nix. Both methods require Rust tooling or Nix
knowledge, which creates adoption friction:

1. **No package manager presence.** Users cannot `brew install`,
   `cargo install context-harness` (from crates.io), or
   `npm install ctx-search` (for the browser widget). The install
   path in the README and HN post requires cloning a Git repo.

2. **No built-in connectors shipped.** The community registry has Lua
   connectors for Jira, Confluence, Slack, and others, but users must
   run `ctx registry init` to get them. Shipping a curated set of
   connectors with the binary would make the out-of-box experience
   richer.

3. **No npm package for ctx-search.js.** The browser search widget
   requires copying files manually. Frontend developers expect
   `npm install` and an import.

4. **Pre-built binaries require manual download.** GitHub releases have
   binaries for six targets, but there is no system-level package
   manager integration.

## Target Users

1. **New users** who want to install Context Harness with a single
   command on their platform of choice.
2. **Frontend developers** who want to add the search widget to their
   site via npm.
3. **Rust developers** who want to depend on `context-harness` or
   `context-harness-core` as a crate from crates.io.
4. **macOS users** who expect `brew install`.

## Goals

1. Publish `context-harness` and `context-harness-core` to crates.io.
2. Publish `ctx-search` (the browser search widget) to npm.
3. Create a Homebrew formula for macOS/Linux installation.
4. Ship a curated set of built-in Lua connectors (Jira, Confluence,
   Slack, GitHub Issues, RSS, Notion) bundled with the binary or
   installed on first run.
5. Maintain pre-built binaries for all six targets via GitHub Releases.

## Non-Goals

- Distribution via Linux distro package managers (apt, dnf, pacman)
  in the initial release. Can be added later based on demand.
- A Windows installer (MSI/NSIS). Windows users can use
  `cargo install` or download the pre-built binary.
- An app store presence (Mac App Store, Microsoft Store).

## User Stories

**US-1: Install via Homebrew.**
A macOS user runs `brew install context-harness`. The formula installs
the pre-built binary. They run `ctx init` and are ready to go.

**US-2: Install via cargo.**
A Rust developer runs `cargo install context-harness` and gets the
latest release from crates.io. No Git clone required.

**US-3: Add search widget via npm.**
A frontend developer runs `npm install @parallax-labs/ctx-search` and
imports the widget in their docs site build. They configure it with a
JSON data file path and get command-K search.

**US-4: Depend on core as a library.**
A Rust developer adds `context-harness-core = "0.x"` to their
`Cargo.toml` and gets access to chunking, search, Store trait, and
data models from crates.io. No Git dependency.

**US-5: Built-in connectors on first run.**
A new user runs `ctx init`. The setup prompt offers to install built-in
connectors for popular services. If they select Jira, the Lua connector
and example config are installed without needing `ctx registry init`.

## Requirements

### crates.io

1. `context-harness-core` SHALL be published to crates.io with proper
   metadata (description, license, repository, categories, keywords).
2. `context-harness` SHALL be published to crates.io as both a library
   and a binary crate.
3. Versions SHALL follow semantic versioning. The workspace refactor
   (PRD-0006) must land before the first crates.io publish.

### npm

4. `ctx-search` SHALL be published to npm under the
   `@parallax-labs` scope (or equivalent).
5. The npm package SHALL include the JavaScript widget, TypeScript
   type definitions, and a README with usage instructions.

### Homebrew

6. A Homebrew formula SHALL be created in a tap repository
   (e.g., `parallax-labs/homebrew-tap`).
7. The formula SHALL download pre-built binaries from GitHub Releases
   for the user's platform.
8. The formula SHALL be updated automatically when new releases are
   tagged (via CI).

### Built-in Connectors

9. The binary SHALL ship with or easily install a curated set of Lua
   connectors: Jira, Confluence, Slack, GitHub Issues, Notion, RSS.
10. Each built-in connector SHALL include an example config snippet
    and a README.
11. Built-in connectors SHALL be overridable via the registry
    precedence model (PRD-0003).

### CI/CD

12. GitHub Actions SHALL automate: building binaries for six targets,
    creating GitHub Releases, publishing to crates.io, publishing to
    npm, and updating the Homebrew formula.

## Success Criteria

- `cargo install context-harness` works from crates.io.
- `brew install parallax-labs/tap/context-harness` works on macOS.
- `npm install @parallax-labs/ctx-search` works and the widget renders.
- A new user can go from zero to searching in <5 minutes using any
  of the three install methods.
- Built-in connectors are discoverable via `ctx registry list` without
  running `ctx registry init`.

## Dependencies and Risks

- **PRD-0006 (Workspace Refactor):** crates.io publishing requires the
  workspace split so `context-harness-core` can be published as a
  standalone library crate.
- **Naming conflicts:** Verify that `context-harness` and `ctx-search`
  are available on crates.io and npm before publishing.
- **Homebrew tap maintenance:** The tap repo must be kept in sync with
  releases. Automate via CI to avoid manual drift.
- **Built-in connector maintenance:** Bundled connectors must be tested
  against live APIs periodically to catch breaking changes from
  third-party services.
- **Semver discipline:** Once published to crates.io, breaking changes
  require major version bumps. The public API must be stable before
  the first publish.

## Related Documents

- **PRDs:** [PRD-0006](0006-workspace-refactor-and-library-publishing.md)
  (prerequisite: workspace split for crates.io),
  [PRD-0002](0002-browser-search-widget.md) (ctx-search.js published
  to npm), [PRD-0003](0003-extensibility-platform.md) (registry model
  for built-in connectors)
- **ADRs:** [0016](../adr/0016-nix-for-builds.md) (Nix builds coexist
  with package manager distribution),
  [0013](../adr/0013-git-backed-extension-registries.md) (registry
  model for built-in connectors)
