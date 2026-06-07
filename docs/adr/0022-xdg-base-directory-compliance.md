# ADR-0022: XDG Base Directory Compliance

**Status:** Proposed
**Date:** 2026-02-28

## Context

Context Harness stores configuration, data, and cache files across several inconsistent locations:

- **Config:** `./config/ctx.toml` (CWD-relative, only works from workspace root)
- **Registries:** `~/.ctx/registries/` (custom dot-directory)
- **Model cache:** `~/.fastembed_cache` or Tauri's `app_data_dir` (varies by surface)
- **Database:** `./data/ctx.sqlite` (CWD-relative, embedded in workspace)
- **Git clone cache:** `<db-dir>/.git-cache/` (mixed with workspace data)

There is no global configuration. Users who work with multiple workspaces must duplicate embedding provider settings, API keys, and retrieval tuning across every workspace's `config/ctx.toml`. There is also no way to set system-wide defaults.

The [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/) is the de facto standard for organizing user-level files on Unix-like systems. It separates concerns into config, data, cache, and state directories, each controlled by environment variables with well-known defaults. Most modern CLI tools in the Rust ecosystem (ripgrep, starship, helix, lazygit, fish) follow this convention.

The existing `./config/ctx.toml` convention must continue to work for backward compatibility. Users who have deployed Context Harness with `--config` flags, Dockerfiles, or systemd units must not be broken.

## Decision

Context Harness adopts XDG Base Directory semantics for all user-level file storage, with full backward compatibility for the existing `./config/ctx.toml` workspace convention.

The specific directory mapping is:

| Category | XDG Variable | Default | ctx Path |
|----------|-------------|---------|----------|
| Config | `$XDG_CONFIG_HOME` | `~/.config` | `~/.config/ctx/config.toml` |
| Data | `$XDG_DATA_HOME` | `~/.local/share` | `~/.local/share/ctx/` |
| Cache | `$XDG_CACHE_HOME` | `~/.cache` | `~/.cache/ctx/` |
| State | `$XDG_STATE_HOME` | `~/.local/state` | `~/.local/state/ctx/` |

Workspace-local configuration uses a `.ctx/` dot-directory within the workspace root instead of the current `config/` directory:

```
my-workspace/.ctx/config.toml    # New convention
my-workspace/config/ctx.toml     # Old convention (still supported)
```

The CLI config resolution follows a layered fallback chain:

```
1. --config <path>                     (explicit — highest priority)
2. $CTX_CONFIG                         (env var override)
3. ./.ctx/config.toml                  (workspace-local dot-directory)
4. ./config/ctx.toml                   (legacy — backward compat)
5. $XDG_CONFIG_HOME/ctx/config.toml    (user global defaults)
6. Built-in defaults                   (compiled-in minimal config)
```

For macOS, the CLI uses `~/.config/`, `~/.cache/`, etc. (the established convention for Unix CLI tools on macOS), not `~/Library/`. The Tauri desktop app continues using Tauri's native directory APIs for its own settings.

## Alternatives Considered

### Keep `./config/ctx.toml` as the only config location

Simple, but provides no global configuration, no standard cache/data separation, and no discoverability. Users in multi-workspace setups must duplicate settings across every workspace. Rejected because it doesn't scale.

### Use `~/.ctx/` as a single dot-directory for everything

This is the partial approach already used for registries (`~/.ctx/registries/`). However, it conflates config, data, and cache in one directory. Users who store `$XDG_CACHE_HOME` on a fast drive or sync `$XDG_CONFIG_HOME` via dotfile managers cannot take advantage of the separation. It also doesn't follow any established standard. Rejected in favor of the widely-adopted XDG separation.

### Use the `directories` crate for platform-native paths

The `directories` crate maps to `~/Library/Preferences/` on macOS and `%APPDATA%` on Windows. This is correct for GUI applications but wrong for CLI tools, where XDG is the expected convention on all Unix-like systems including macOS. Every major Rust CLI tool uses XDG on macOS. Rejected for the CLI; the desktop app already uses Tauri's native paths and continues to do so.

### Use a single `~/.ctxrc` file (similar to `~/.bashrc`)

A single file for global config is simple but doesn't address data, cache, or state separation. It also doesn't follow XDG and would need to coexist with the TOML config format. Rejected.

## Consequences

### Positive

- **Global defaults.** Users configure embedding providers, API keys, and retrieval tuning once in `~/.config/ctx/config.toml` and every workspace inherits those settings.
- **Standard locations.** `~/.cache/ctx/` can be cleared without affecting config. `~/.config/ctx/` can be synced via dotfile managers. Docker can mount specific directories.
- **Discoverability.** `ctx config path` can print where config is loaded from. Users don't need to guess.
- **Clean workspaces.** `.ctx/` is hidden by default and follows the `.git/`, `.vscode/`, `.cursor/` convention. Projects with their own `config/` directory won't conflict.

### Negative

- **Two config locations during transition.** Both `.ctx/config.toml` and `config/ctx.toml` will be checked. This adds a small amount of complexity to the resolution logic. The fallback chain is well-defined and deterministic.
- **Documentation churn.** Runbooks, README, examples, and the Dockerfile all reference `config/ctx.toml` and must be updated to show the new `.ctx/config.toml` as the primary location while noting backward compatibility.
- **macOS divergence from Tauri.** The CLI uses `~/.config/ctx/` while the desktop app uses `~/Library/Application Support/com.context-harness.app/`. This is intentional (CLI vs GUI conventions) but means they read settings from different places. The core library provides a unified API that abstracts this.

### Constraints

- The `--config` flag and `$CTX_CONFIG` env var must continue to override all automatic resolution.
- The TOML config file format is unchanged.
- Existing Dockerfiles using `COPY config/ /app/config/` and `--config /app/config/ctx.toml` must continue to work without modification.

## References

- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/)
- [DESIGN-0007](../design/0007-xdg-config-directories.md) — Design exploration and implementation plan
- [SPEC-0013](../spec/0013-config-resolution.md) — Authoritative config resolution spec
- [SPEC-0005](../spec/0005-usage-contract.md) — Current usage contract
- [ADR-0010](0010-toml-configuration-with-env-expansion.md) — TOML config with env expansion
