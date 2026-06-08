# DESIGN-0007: XDG-Compliant Config and Data Directories

**Status:** Planning
**Date:** 2026-02-28
**Author:** pjones
**Related:** [PRD-0011](../prd/0011-native-app.md), [ADR-0010](../adr/0010-toml-configuration-with-env-expansion.md), [SPEC-0005](../spec/0005-usage-contract.md), [SPEC-0013](../spec/0013-config-resolution.md), [ADR-0022](../adr/0022-xdg-base-directory-compliance.md)

## Context

Context Harness currently stores files across several ad-hoc locations:

| Artifact | Current Location | Problem |
|----------|------------------|---------|
| CLI config | `./config/ctx.toml` (CWD-relative) | Only works from the repo root; no global config |
| SQLite database | `./data/ctx.sqlite` (CWD-relative) | Embedded in workspace; no shared default |
| Extension registries | `~/.ctx/registries/...` | Custom dot-directory; doesn't follow XDG |
| Fastembed model cache | `~/.fastembed_cache` or Tauri `app_data_dir` | Inconsistent between CLI and desktop app |
| Tauri app settings | `~/Library/Application Support/com.context-harness.app/` | macOS-specific via Tauri APIs |
| Git connector cache | `<db-dir>/.git-cache/<hash>` | Mixed with workspace data |

This causes several problems:

1. **No global config.** Users must pass `--config` or run from a workspace root. There is no way to set global defaults (default embedding provider, API keys, preferred server bind address) that apply to all workspaces.

2. **Registry path is non-standard.** `~/.ctx/` is a custom dot-directory that doesn't respect `$XDG_DATA_HOME`.

3. **Cache is scattered.** Fastembed models, git clones, and embedding caches are stored in different places depending on whether you're using the CLI or the desktop app.

4. **No discoverability.** A new user has no way to find where Context Harness stores things without reading the source code.

The [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/) solves this by defining standard directories for config, data, cache, and state. Most modern CLI tools (ripgrep, starship, lazygit, helix, etc.) follow it.

## Proposal

Adopt XDG Base Directory semantics for all Context Harness file storage, with backward-compatible fallback to the current `./config/ctx.toml` workspace-relative convention.

### Directory Mapping

| XDG Variable | Default (Linux) | Default (macOS) | ctx Usage |
|-------------|-----------------|-----------------|-----------|
| `$XDG_CONFIG_HOME` | `~/.config` | `~/.config` | Global config: `~/.config/ctx/config.toml` |
| `$XDG_DATA_HOME` | `~/.local/share` | `~/.local/share` | Registries, installed extensions: `~/.local/share/ctx/` |
| `$XDG_CACHE_HOME` | `~/.cache` | `~/.cache` | Model cache, git clone cache: `~/.cache/ctx/` |
| `$XDG_STATE_HOME` | `~/.local/state` | `~/.local/state` | Logs, history: `~/.local/state/ctx/` |

**macOS note:** For the CLI tool, we follow XDG conventions directly (using `~/.config`, `~/.cache`, etc.) rather than macOS `~/Library/` paths. This is the established convention for Unix CLI tools on macOS (ripgrep, starship, fish, helix all do this). The Tauri desktop app will continue using Tauri's native directory APIs (`~/Library/Application Support/...`) for its own settings, while the core library uses XDG.

### Directory Layout

```
~/.config/ctx/
├── config.toml              # Global config (defaults for all workspaces)
└── workspaces.toml          # Workspace registry (known workspace paths)

~/.local/share/ctx/
├── registries/
│   └── community/           # Git-backed extension registry
├── extensions/               # Locally installed extensions
└── default/
    └── data/
        └── ctx.sqlite       # Default workspace database

~/.cache/ctx/
├── models/
│   └── fastembed/            # ONNX embedding model cache
├── git/
│   └── <hash>/              # Git connector clone cache
└── embeddings/               # Pre-computed embedding cache

~/.local/state/ctx/
└── logs/
    └── ctx.log               # Runtime log output
```

### Config Resolution Order

The CLI SHALL resolve configuration using a layered fallback:

```
1. --config <path>                     (explicit flag — highest priority)
2. $CTX_CONFIG                         (env var override)
3. ./.ctx/config.toml                  (workspace-local dot-directory)
4. ./config/ctx.toml                   (legacy workspace-local — backward compat)
5. $XDG_CONFIG_HOME/ctx/config.toml    (user global config)
6. Built-in defaults                   (compiled-in minimal config)
```

Workspace-local config is merged with global defaults when both exist; the workspace file overrides only the keys it specifies. Explicit path sources (`--config` and `$CTX_CONFIG`) bypass the merge chain entirely (current behavior preserved).

### Workspace-Local Config: `.ctx/` Directory

Within a workspace (project directory), the config moves from `config/ctx.toml` to `.ctx/config.toml`:

```
my-project/
├── .ctx/
│   ├── config.toml           # Workspace config
│   ├── data/
│   │   └── ctx.sqlite        # Workspace database
│   └── extensions/           # Workspace-local extensions (overrides)
├── docs/
├── src/
└── README.md
```

**Why `.ctx/` instead of `config/`:**

- Follows the dot-directory convention used by `.git/`, `.vscode/`, `.cursor/`, `.cargo/`, `.env`
- Keeps workspace metadata hidden from directory listings by default
- Avoids conflict with projects that have their own `config/` directory
- Groups all ctx artifacts (config + data + extensions) in one place
- Clear ownership: everything in `.ctx/` belongs to Context Harness

### Backward Compatibility

The default layout is backward-compatible:

1. **`./config/ctx.toml` is still found.** The resolution chain checks both `.ctx/config.toml` and `config/ctx.toml`. The old location works indefinitely.

2. **`--config` flag unchanged.** Explicit paths work exactly as before.

3. **`~/.ctx/registries/` still works as a read fallback.** New installs use `$XDG_DATA_HOME/ctx/registries/`, but existing legacy registry trees are still found when the XDG registry directory does not exist.

4. **No breaking changes to `ctx.toml` format.** The TOML schema is unchanged. Only the search paths for finding the config file change.

5. **`ctx init` creates `.ctx/` for new workspaces.** Existing workspaces with `config/ctx.toml` continue to work.

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `CTX_CONFIG` | Override config file path | (none — use resolution chain) |
| `CTX_CONFIG_DIR` | Override config directory | `$XDG_CONFIG_HOME/ctx` |
| `CTX_DATA_DIR` | Override data directory | `$XDG_DATA_HOME/ctx` |
| `CTX_CACHE_DIR` | Override cache directory | `$XDG_CACHE_HOME/ctx` |
| `CTX_STATE_DIR` | Override state directory | `$XDG_STATE_HOME/ctx` |
| `XDG_CONFIG_HOME` | XDG config base | `~/.config` |
| `XDG_DATA_HOME` | XDG data base | `~/.local/share` |
| `XDG_CACHE_HOME` | XDG cache base | `~/.cache` |
| `XDG_STATE_HOME` | XDG state base | `~/.local/state` |

`CTX_*` variables take precedence over `XDG_*` variables, which take precedence over defaults. This allows Docker containers and CI to override the relevant path categories directly.

## Alternatives Considered

### 1. Keep `./config/ctx.toml` only

**Rejected.** No global config, no standard locations, no discoverability. Users in multi-workspace setups must duplicate config.

### 2. Use `~/.ctx/` for everything (current partial approach)

**Rejected.** Conflates config, data, and cache in one directory. Doesn't respect XDG. Users who set `XDG_CACHE_HOME` to a fast drive or `XDG_CONFIG_HOME` to a synced directory can't take advantage of that separation.

### 3. Use macOS `~/Library/` paths on macOS

**Rejected for CLI.** The convention for Unix CLI tools on macOS is to use `~/.config/` etc. The Tauri desktop app already uses `~/Library/Application Support/` via Tauri APIs and will continue to do so. The core library and CLI use XDG.

### 4. Use the `directories` crate for platform-specific paths

**Considered.** The `directories` crate maps to platform-native paths (`~/Library/Preferences/` on macOS, `%APPDATA%` on Windows). This is correct for GUI applications but wrong for CLI tools, where XDG is the expected convention even on macOS. We use the `dirs` crate for home directory detection and implement XDG resolution directly.

## Implementation Plan

1. **Add `ctx_dirs` module to the app crate.** Exposes functions for workspace `.ctx/` paths and XDG config/data/cache/state paths. Checks env vars, applies XDG defaults.

2. **Update `load_config` in `config.rs`.** When no `--config` flag is provided, use the resolution chain instead of the hardcoded `./config/ctx.toml` default.

3. **Update CLI `--config` default.** Change from `default_value = "./config/ctx.toml"` to using the resolution chain. The flag still overrides everything.

4. **Update registry path.** Change `~/.ctx/registries/` to `data_dir()/registries/` with fallback to the old location.

5. **Update local model cache.** Change fastembed and tract local model caches to `cache_dir()/models/` with env var fallback.

6. **Reserve state paths for non-analytics CLI state.** Keep `state_dir()` available for future logs or history, but do not create telemetry state.

7. **Keep desktop/native path handling as follow-up.** This PR enforces CLI/app-crate defaults; any desktop-specific workspace creation should adopt the same `.ctx/` layout later.

8. **Update docs, runbooks, and example configs.**

9. **Write SPEC-0013 and ADR-0022.** Lock down the behavior and record the decision.

## Resolved Decisions

1. **Global config is not auto-created on first run.** `ctx init` bootstraps workspace-local `.ctx/config.toml` for new workspaces.

2. **Workspace-local `.ctx/` gets its own `.gitignore`.** Generated workspaces ignore `.ctx/data/` and `.ctx/cache/`; `.ctx/config.toml` remains visible for versioning if desired.

3. **Config merge semantics are deep for tables.** Workspace config inherits global config and overrides only specified keys. Arrays are replaced.

4. **Docker and explicit paths remain supported.** `--config`, `CTX_CONFIG`, and legacy `config/ctx.toml` preserve existing scripted/test usage.
