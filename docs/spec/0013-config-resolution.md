# SPEC-0013: Config Resolution and Directory Layout

**Status:** Draft
**Date:** 2026-02-28
**Scope:** Configuration file discovery, directory layout for config/data/cache/state, backward compatibility with `config/ctx.toml`.

## Overview

This spec defines how Context Harness locates configuration files, where it stores data and cache artifacts, and the environment variables that control these paths. It replaces the implicit `./config/ctx.toml` default with a deterministic, layered resolution chain that supports global defaults, workspace-local overrides, and explicit path flags.

## Definitions

- **Global config:** User-level configuration that applies to all workspaces. Stored under `$XDG_CONFIG_HOME/ctx/`.
- **Workspace config:** Per-project configuration. Stored in `.ctx/config.toml` (or legacy `config/ctx.toml`) within the workspace root.
- **Effective config:** The result of merging global config with workspace config. This is what the runtime uses.
- **Workspace root:** The directory containing `.ctx/` or `config/ctx.toml`, or the directory passed to `ctx --config` or `workspace_open`.

## 1. XDG Directory Mapping

Context Harness SHALL use the following XDG-based directories:

| Category | Env Override | XDG Variable | Default |
|----------|-------------|-------------|---------|
| Config | `CTX_CONFIG_DIR` | `XDG_CONFIG_HOME` | `~/.config/ctx` |
| Data | `CTX_DATA_DIR` | `XDG_DATA_HOME` | `~/.local/share/ctx` |
| Cache | `CTX_CACHE_DIR` | `XDG_CACHE_HOME` | `~/.cache/ctx` |
| State | `CTX_STATE_DIR` | `XDG_STATE_HOME` | `~/.local/state/ctx` |

### R1: Resolution Order for Directories

For each category, the directory SHALL be resolved in this order:

1. `CTX_*_DIR` env var (e.g., `CTX_CACHE_DIR=/fast/cache/ctx`)
2. `$XDG_*_HOME/ctx` (e.g., `$XDG_CACHE_HOME/ctx`)
3. Platform default (e.g., `~/.cache/ctx`)

All paths MUST be absolute. Relative paths in environment variables SHALL be treated as invalid and ignored (per XDG spec).

### R2: Platform Defaults

On all Unix-like systems (Linux, macOS, BSDs):

| Directory | Default |
|-----------|---------|
| Config | `$HOME/.config/ctx` |
| Data | `$HOME/.local/share/ctx` |
| Cache | `$HOME/.cache/ctx` |
| State | `$HOME/.local/state/ctx` |

The CLI SHALL use these XDG paths on macOS rather than `~/Library/` paths. The Tauri desktop app MAY use Tauri's native `app_data_dir()` for its own application settings but SHALL use the core library's XDG paths for shared artifacts (registries, model cache).

## 2. Global Directory Layout

```
$XDG_CONFIG_HOME/ctx/              # Config
├── config.toml                    # Global defaults
└── workspaces.toml                # Known workspace registry

$XDG_DATA_HOME/ctx/                # Data
├── registries/
│   └── community/                 # Git-backed extension registry
└── extensions/                    # User-installed extensions

$XDG_CACHE_HOME/ctx/               # Cache
├── models/
│   └── fastembed/                 # ONNX embedding model cache
└── git/
    └── <hash>/                    # Git connector clone cache

$XDG_STATE_HOME/ctx/               # State
└── logs/
    └── ctx.log                    # Runtime log
```

### R3: Global Config File

The global config file SHALL be located at `$XDG_CONFIG_HOME/ctx/config.toml`. It uses the same TOML schema as workspace configs. Any section present in the global config serves as a default for all workspaces.

### R4: Directory Auto-Creation

Context Harness SHALL create directories on first write, not on startup. It SHALL NOT create empty directories speculatively.

## 3. Workspace Directory Layout

### R5: Workspace Config Location

A workspace MAY store its configuration in a `.ctx/` directory at its root:

```
workspace-root/
├── .ctx/
│   ├── config.toml               # Workspace config
│   ├── data/
│   │   └── ctx.sqlite            # Workspace database
│   └── .gitignore                # Ignores data/ by default
├── docs/
└── src/
```

### R6: Legacy Config Location

For backward compatibility, Context Harness SHALL also recognize `config/ctx.toml` within a workspace root:

```
workspace-root/
├── config/
│   └── ctx.toml                  # Legacy workspace config
├── data/
│   └── ctx.sqlite                # Legacy database location
└── docs/
```

### R7: Priority Between `.ctx/` and `config/`

When both `.ctx/config.toml` and `config/ctx.toml` exist in the same workspace, `.ctx/config.toml` SHALL take priority. A warning MAY be emitted to stderr noting the ambiguity.

### R8: Workspace `.gitignore`

When `ctx init` creates a `.ctx/` directory, it SHALL create a `.ctx/.gitignore` file containing:

```
data/
```

This ensures the database is not committed while allowing `config.toml` to be version-controlled.

## 4. Config File Resolution Chain

### R9: CLI Resolution

When the CLI is invoked without `--config`, it SHALL search for a config file in this order:

| Priority | Source | Path |
|----------|--------|------|
| 1 | `--config` flag | Exact path provided |
| 2 | `CTX_CONFIG` env var | Exact path from env |
| 3 | Workspace dot-dir | `./.ctx/config.toml` |
| 4 | Legacy workspace dir | `./config/ctx.toml` |
| 5 | Global config | `$XDG_CONFIG_HOME/ctx/config.toml` |
| 6 | Built-in defaults | Compiled-in minimal config |

The search stops at the first file that exists and is readable. Priorities 1 and 2 are absolute — they bypass all auto-detection.

### R10: Config Merge Semantics

When a workspace config is found (priorities 3 or 4) AND a global config exists (priority 5), they SHALL be merged:

- The global config provides default values for all sections.
- The workspace config overrides keys present in both.
- Merge is **deep**: nested tables (e.g., `[connectors.filesystem.docs]`) are merged at the key level, not replaced wholesale.
- Arrays (e.g., `include_globs`) are replaced, not appended.

When `--config` or `CTX_CONFIG` is used (priorities 1 or 2), no merge occurs. The specified file is the sole config source (current behavior preserved).

### R11: Config Path Reporting

The CLI SHALL support `ctx config path` which prints the resolved config file path and the merge sources:

```
$ ctx config path
Workspace config: /home/user/my-project/.ctx/config.toml
Global config:    /home/user/.config/ctx/config.toml
Effective:        (merged)
```

## 5. Environment Variables

### R12: Environment Variable Summary

| Variable | Purpose | Default |
|----------|---------|---------|
| `CTX_CONFIG` | Override config file path (bypasses resolution chain) | (none) |
| `CTX_CONFIG_DIR` | Override config directory | `$XDG_CONFIG_HOME/ctx` |
| `CTX_DATA_DIR` | Override data directory | `$XDG_DATA_HOME/ctx` |
| `CTX_CACHE_DIR` | Override cache directory | `$XDG_CACHE_HOME/ctx` |
| `CTX_STATE_DIR` | Override state directory | `$XDG_STATE_HOME/ctx` |
| `FASTEMBED_CACHE_DIR` | Override fastembed model cache | `$CTX_CACHE_DIR/models/fastembed` |

`CTX_*` variables take precedence over `XDG_*` variables. This allows Docker containers and CI environments to override all paths with a single prefix.

### R13: Env Var Validation

All path environment variables SHALL be validated:

- Paths MUST be absolute. Relative paths are ignored with a warning to stderr.
- Non-existent paths are acceptable (created on first write per R4).
- Empty values are treated as unset.

## 6. Backward Compatibility

### R14: Existing `./config/ctx.toml` Workspaces

Workspaces using `config/ctx.toml` SHALL continue to work without modification. The resolution chain (R9) checks this location at priority 4.

### R15: Existing `--config` Flag

The `--config` flag SHALL continue to accept any path and bypass all automatic resolution. This preserves compatibility with:

- Dockerfiles using `--config /app/config/ctx.toml`
- systemd units using `--config /opt/context-harness/config/ctx.toml`
- CI scripts using `--config ./custom/path.toml`

### R16: Existing Registry Path

If `$XDG_DATA_HOME/ctx/registries/` does not exist but `~/.ctx/registries/` does, the latter SHALL be used as a fallback. This preserves compatibility with existing registry installations.

### R17: Migration Command

The CLI SHALL provide `ctx migrate` which:

1. Detects `config/ctx.toml` and offers to move it to `.ctx/config.toml`
2. Detects `data/` and offers to move it to `.ctx/data/`
3. Detects `~/.ctx/registries/` and offers to move it to `$XDG_DATA_HOME/ctx/registries/`
4. Creates `~/.config/ctx/config.toml` with commented defaults if it doesn't exist

The command SHALL be interactive by default and support `--dry-run` and `--yes` flags.

## 7. Init Command Changes

### R18: `ctx init` Behavior

When `ctx init` is run in a directory without `--config`:

1. If `.ctx/config.toml` exists, use it.
2. If `config/ctx.toml` exists, use it.
3. Otherwise, create `.ctx/config.toml` and `.ctx/data/` (new workspace).

### R19: `ctx init --global`

`ctx init --global` SHALL create `$XDG_CONFIG_HOME/ctx/config.toml` with commented defaults if it doesn't exist.

## Acceptance Criteria

1. `ctx --config ./explicit.toml sync all` works exactly as before.
2. `CTX_CONFIG=/path/to/config.toml ctx sync all` uses the specified file.
3. Running `ctx sync all` from a workspace with `.ctx/config.toml` uses that file.
4. Running `ctx sync all` from a workspace with only `config/ctx.toml` uses that file.
5. Global config at `~/.config/ctx/config.toml` provides defaults for workspace configs.
6. `ctx config path` prints the resolved config source(s).
7. `ctx migrate --dry-run` detects old locations and reports planned moves.
8. Registries in `~/.ctx/registries/` are found when `$XDG_DATA_HOME/ctx/registries/` doesn't exist.
9. Fastembed models are cached in `$XDG_CACHE_HOME/ctx/models/fastembed/` by default.
10. Docker deployments using `--config /app/config/ctx.toml` work without changes.
