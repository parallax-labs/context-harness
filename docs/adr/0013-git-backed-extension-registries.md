# ADR-0013: Git-Backed Extension Registries

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness supports extensibility through Lua-scripted connectors,
tools, and agents (see [ADR-0008](0008-lua-for-runtime-extensibility.md)).
Users and communities need a way to share and distribute these extensions
without requiring a centralized package infrastructure.

Requirements:

- Extensions should be discoverable and installable from shared repositories
- Multiple registries should coexist with clear precedence (team overrides
  community, project overrides team)
- No centralized server or hosting infrastructure required
- Version compatibility between extensions and the host binary
- Project-local extensions (`.ctx/` directory) should be auto-discovered

## Decision

Use **Git repositories as extension registries**, inspired by the
[cheat/cheat](https://github.com/cheat/cheat) cheatpath system.

### Registry Structure

Each registry is a Git repository containing:

```
registry.toml           # manifest with metadata and extension entries
connectors/
  <name>/connector.lua
tools/
  <name>/tool.lua
agents/
  <name>/agent.lua
```

The `registry.toml` manifest declares:

- Registry metadata (name, description, URL)
- Extension entries with description, path, tags, required config, host APIs
- Optional `min_version` for compatibility checking

### Precedence

When multiple registries provide an extension with the same name, precedence
is (highest to lowest):

1. **Explicit config** — extensions defined directly in `ctx.toml`
2. **Project-local `.ctx/`** — auto-discovered by walking CWD ancestors
3. **Personal registry** — user's own extensions
4. **Company registry** — organization-shared extensions
5. **Community registry** — open-source community extensions

### Discovery Rules

- **Tools and agents** from registries are auto-discovered and available
  immediately without additional configuration.
- **Connectors** from registries are discovered but **not active** until
  explicitly added via `ctx registry add <name>`, because connectors
  typically require credentials and other configuration.

### CLI Commands

```
ctx registry list       # list installed registries and their extensions
ctx registry install    # install a registry from a Git URL
ctx registry update     # git pull all registries
ctx registry search     # search for extensions across registries
ctx registry info       # show details for a specific extension
ctx registry add        # activate a registry connector
ctx registry override   # copy an extension locally for customization
ctx registry init       # scaffold a new registry
```

### First-Run Experience

`ctx init` can prompt to install the community registry when run
interactively, providing a curated set of connectors and tools out of
the box.

## Alternatives Considered

**Package manager (npm, crates.io).** A centralized package registry provides
versioning, dependency resolution, and discoverability. However, it requires
infrastructure (registry server, publishing pipeline, accounts), is
heavyweight for Lua scripts, and creates a single point of failure. Git
repositories are simpler and leverage existing infrastructure (GitHub,
GitLab).

**HTTP API registry.** A REST API that serves extension manifests and files.
Requires hosting and maintenance. Git repositories are self-hosting and
work with any Git provider (including self-hosted Gitea/GitLab).

**Single monorepo.** All community extensions in one repository. Simple but
does not scale — every user clones all extensions, merge conflicts arise
between contributors, and there is no precedence mechanism.

**Filesystem-only (no Git).** Users manually copy extension files into a
directory. Works but provides no versioning, no update mechanism, and no
discoverability. Git adds `clone`, `pull`, and history for free.

**OCI/container registry.** Store extensions as container image layers.
Powerful but extremely heavyweight for distributing Lua scripts. The tooling
(Docker, ORAS) is complex relative to `git clone`.

## Consequences

- Extension distribution requires only a Git repository — no specialized
  infrastructure, accounts, or publishing pipelines.
- The cheatpath-inspired precedence model allows organizations to override
  community extensions with internal versions, and projects to override
  both with local customizations.
- Auto-discovery of `.ctx/` directories makes project-specific extensions
  work without any configuration, similar to how `.git/` is auto-discovered.
- `min_version` in `registry.toml` provides basic compatibility checking
  without a full semver resolution system.
- Connector activation via `ctx registry add` is a deliberate safety
  measure — connectors execute code and need credentials, so they should
  not activate silently when a registry is installed.
- Git operations (clone, pull) require `git` to be installed on the system.
  This is a reasonable assumption for the developer audience but is a
  dependency.
- The `ctx registry override` command supports a "fork and customize"
  workflow: copy an extension locally, modify it, and have the local version
  take precedence over the registry version.
