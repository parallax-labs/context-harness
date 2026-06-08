+++
title = "v0.7.0: Faster Semantic Search, XDG Config, and No Telemetry"
description = "Context Harness v0.7.0 adds the zvec vector-index sidecar, formal storage boundaries, XDG-friendly config paths, a cleaner docs hierarchy, and removes telemetry entirely."
date = 2026-06-08

[taxonomies]
tags = ["release"]
+++

Context Harness v0.7.0 is about making the project feel less like a promising CLI and more like local-first infrastructure you can trust: faster semantic retrieval when you have embeddings, cleaner storage boundaries, predictable config locations, organized project docs, and no telemetry.

The release pulls together the work from v0.6.0 through today: the workspace split and documentation foundation, the vector-index design work, XDG config compliance, the zvec sidecar implementation, and the telemetry removal.

---

### Release notes

The latest release is **v0.7.0**, published on June 8, 2026.

**What's changed since v0.6.0:**

- Organized the project documentation hierarchy into ADRs, PRDs, specs, designs, and runbooks.
- Added the vector-index acceleration design and storage boundary spec.
- Added XDG config and directory compliance while keeping legacy `config/ctx.toml` compatibility.
- Added the optional zvec vector-index sidecar behind the `zvec-bundled` feature.
- Removed telemetry and the PostHog dependency entirely.
- Published release binaries for Linux, macOS, and Windows with checksum files.

**Download:**

```bash
# Apple Silicon macOS
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# Linux x86_64
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-linux-x86_64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/
```

Windows users can download `ctx-windows-x86_64.zip` from the [v0.7.0 release](https://github.com/parallax-labs/context-harness/releases/tag/v0.7.0).

---

### Faster semantic search with zvec

Context Harness still uses SQLite as the canonical local store: documents, chunks, checkpoints, FTS5 keyword search, embedding metadata, stats, and export all live there. v0.7.0 adds a separate optional acceleration layer for semantic search: a rebuildable **zvec vector-index sidecar**.

That means the storage model is now explicit:

```
.ctx/data/
  ctx.sqlite             # canonical store
  vector-index/zvec/     # derived semantic-search sidecar
```

The sidecar is derived state. If it is missing, stale, or unavailable, Context Harness can rebuild it from SQLite or fall back to the exact SQLite vector scan. Keyword search continues to use SQLite FTS5.

The benchmark that motivated the sidecar is stark: on a real corpus shape with 4,917 chunks, zvec HNSW candidate retrieval measured around 0.8 ms median compared with roughly 112 ms for the exact SQLite scan, with high top-k overlap against the SQLite baseline. The full write-up is in [Vector Search Bake-Off: SQLite Brute Force vs zvec](/blog/vector-search-bakeoff-zvec/).

The important design point is not "replace SQLite." It is: **SQLite owns truth; zvec owns speed.**

---

### Storage boundaries are now part of the contract

v0.7.0 introduces the app-level `AppStore` and optional `VectorIndex` boundary. This is plumbing, but it matters: the reusable core search flow can keep working against the `Store` trait, while the native application owns operational storage concerns like initialization, checkpoints, embedding maintenance, stats, and export.

The new boundary keeps the responsibilities sharp:

- `SqliteAppStore` owns canonical app storage and operational maintenance.
- `SqliteStore` remains the search-facing SQLite implementation.
- `VectorIndex` owns optional semantic candidate retrieval only.
- zvec is allowed to accelerate semantic search, but not become authoritative for documents, chunks, keyword search, stats, or export.

This gives Context Harness room to improve retrieval performance without turning the canonical local database into a grab bag of special cases.

---

### XDG config and cleaner workspace state

Context Harness now has a clearer directory model for CLI users. New workspaces prefer `.ctx/config.toml`, `.ctx/data/`, and `.ctx/cache/`; global defaults can live under the usual XDG paths such as `~/.config/ctx/config.toml`.

The config resolution order is designed to be predictable and backward-compatible:

1. `--config <path>`
2. `CTX_CONFIG`
3. `./.ctx/config.toml`
4. `./config/ctx.toml`
5. `$XDG_CONFIG_HOME/ctx/config.toml`
6. built-in defaults

Existing deployments that pass `--config` or use `config/ctx.toml` keep working. The change gives new projects a less noisy default and gives multi-workspace users a place for global defaults.

---

### Telemetry removed

v0.7.0 removes product telemetry entirely. No analytics client, no product event stream, no local analytics identity, and no PostHog dependency.

Context Harness often indexes source code, project notes, runbooks, planning docs, and other private context. Even anonymous analytics creates a networked reporting path that weakens the local-first trust boundary. The rule is simpler now:

**Context Harness will stay telemetry-free forever.**

Network access still happens when you explicitly configure networked behavior, such as Git, S3, OpenAI embeddings, Ollama, model downloads, or registry installation. Product telemetry is different, and it is gone. The longer explanation is in [Telemetry-free forever](/blog/telemetry-free-forever/).

---

### Documentation grew up

The docs are now organized around how the project is actually built:

- **ADRs** for decisions.
- **PRDs** for product requirements.
- **Specs** for authoritative contracts.
- **Designs** for implementation plans.
- **Runbooks** for operational workflows.

That structure is already paying off. The vector-index work has an explicit storage contract, the XDG work has a config-resolution spec, and release/debug workflows have places to live besides issue comments and memory.

---

### Upgrading

Use the latest release binary for your platform, or install from the repo:

```bash
# From a release asset
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# From source
cargo install --path crates/context-harness --force
```

No config migration is required. Existing `--config` and `config/ctx.toml` workflows remain supported. New workspaces can move toward `.ctx/config.toml` when convenient.

---

### What this release means

v0.7.0 is not just a feature drop. It tightens the shape of Context Harness:

- faster semantic search without abandoning SQLite
- clearer boundaries between canonical storage and accelerators
- standard config/data/cache locations
- better docs for contributors and operators
- a stronger privacy promise

That is the direction from here: local-first context infrastructure that stays boring where it should be boring, fast where speed matters, and honest about where your context goes.
