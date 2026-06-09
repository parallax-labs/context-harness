+++
title = "v0.8.0: Static zvec Release Binaries"
description = "Context Harness v0.8.0 makes zvec-backed semantic search part of the native release binary story, fixes hyphenated keyword queries, and tightens CI coverage for the vector index path."
date = 2026-06-09

[taxonomies]
tags = ["release"]
+++

Context Harness v0.8.0 is a packaging and reliability release for the vector-index work that landed in v0.7.0.

The headline: the native Linux and Apple Silicon release binaries now build with the zvec vector-index accelerator statically included. You can download `ctx`, run it, and use the zvec-backed semantic search path without manually compiling or installing a separate zvec library.

That matters because Context Harness is supposed to be local-first infrastructure, not a weekend of linker archaeology. The canonical store is still SQLite. The zvec index is still rebuildable sidecar state. v0.8.0 makes the fast path much easier to actually use from the release artifacts.

---

### Release notes

The latest release is **v0.8.0**, published on June 8, 2026.

**What's changed since v0.7.0:**

- Switched the zvec dependency to a forked `zvec-bindings` build that supports static linking.
- Enabled `zvec-bundled` in native release binaries for Linux x86_64, Linux aarch64, and macOS aarch64.
- Added CI coverage for the zvec bundled build, clippy, tests, and native release binary matrix.
- Fixed keyword and hybrid search for user queries containing hyphenated terms like `local-first`, `MCP-compatible`, and `multi-repo`.
- Updated the zvec adapter for the binding API used by the static build path.
- Bumped the CLI and core crates to `0.8.0`.

**Download:**

```bash
# Apple Silicon macOS
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# Linux x86_64
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-linux-x86_64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/
```

Windows users can download `ctx-windows-x86_64.zip` from the [v0.8.0 release](https://github.com/parallax-labs/context-harness/releases/tag/v0.8.0).

---

### zvec in the binary, not beside it

v0.7.0 introduced the zvec vector-index sidecar as the fast semantic candidate path. v0.8.0 turns that into something users can get from a normal release download.

The release workflow now builds these native targets with `local-embeddings-tract,zvec-bundled`:

- `ctx-linux-x86_64.tar.gz`
- `ctx-linux-aarch64.tar.gz`
- `ctx-macos-aarch64.tar.gz`

Those binaries use the statically linked zvec build path. The release smoke check for macOS arm64 verified `ctx 0.8.0` from the downloaded archive and confirmed there is no external `libzvec` dynamic-library dependency.

For the cross or fallback targets, Context Harness still publishes working binaries without zvec bundled:

- `ctx-linux-x86_64-musl.tar.gz`
- `ctx-macos-x86_64.tar.gz`
- `ctx-windows-x86_64.zip`

Those remain useful compatibility artifacts, but the native Linux and Apple Silicon downloads are now the best path if you want accelerated semantic search.

---

### Hyphenated search now behaves like user text

SQLite FTS5 treats punctuation as query syntax in places where normal users just mean words. Before this release, a query containing terms like `local-first` or `MCP-compatible` could be parsed too literally by FTS and fail to return the expected keyword candidates.

v0.8.0 normalizes user-entered keyword text before passing it to FTS:

```
local-first MCP-compatible multi-repo
```

becomes a normal keyword query over:

```
local first MCP compatible multi repo
```

That fix applies to keyword search directly and to the keyword side of hybrid search. The semantic side is unchanged.

---

### CI now proves the zvec path

The release is not just a Cargo feature rename. CI now separately checks:

- default-feature clippy
- `zvec-bundled` clippy
- default-feature tests
- `zvec-bundled` tests
- native release builds with zvec enabled

The release workflow uses the same native zvec feature path for Linux x86_64, Linux aarch64, and macOS aarch64. That gives us a practical confidence gate before publishing downloads.

---

### Upgrading

Use the latest release binary for your platform:

```bash
# Apple Silicon macOS
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# Linux x86_64
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-linux-x86_64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/
```

If you build from source, use the release branch or tag:

```bash
git checkout v0.8.0
cargo install --path crates/context-harness --force
```

No config migration is required. Existing SQLite stores and zvec sidecars continue to follow the same model: SQLite owns the canonical data, and the vector index is derived state that can be rebuilt.

---

### What this release means

v0.8.0 is the release where the vector-index accelerator stops being only an implementation milestone and becomes part of the normal download experience.

There is still more to do: upstreaming the static zvec binding changes, broadening zvec support across fallback targets where it makes sense, and continuing to tune retrieval quality. But the core user-facing promise is now much stronger:

download one native binary, point it at your local context, and get the fast semantic path without a separate zvec install.
