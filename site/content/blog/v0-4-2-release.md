+++
title = "v0.4.2: The Extension Registry Is Here"
description = "The big release: a community extension registry inspired by cheat/cheat — one command, dozens of connectors and tools. Plus local embeddings on every binary and the build fixes that made it ship."
date = 2026-02-26

[taxonomies]
tags = ["release"]
+++

Context Harness v0.4.2 is the first release since v0.3.0 — and the headline is the **extension registry**. One command gives you a Git-backed catalog of connectors, tools, and agents (Jira, Slack, RSS, web-fetch, incident-responder, and more). We designed it after [cheat/cheat](https://github.com/cheat/cheat): a read-only community repo, clear precedence, and local overrides so you can customize without forking. That’s the main dish.

We already advertised **local embeddings** — and they’re still there. What’s new is that *every* pre-built binary now includes them (fastembed on primary platforms, tract on musl and Intel Mac), thanks to the build fixes that finally made v0.4.0 and v0.4.1’s intended releases actually ship. So: **registry** = the new feature; **local embeddings everywhere** = the payoff of the plumbing work. This post is a deep dive on both, with cheat front and center.

---

### The main dish: community extension registry

The **extension registry** is a Git-backed directory of Lua scripts: connectors (ingest data), tools (actions your AI agent can call), and agents (prompts + tool lists). Install a registry once and you get a catalog you can search, add, and override from the CLI — no copy-pasting repos, no manual path wiring.

**One command:**

```bash
ctx registry init
```

That clones the [community registry](https://github.com/parallax-labs/ctx-registry) and adds it to your config. You immediately get access to connectors (Jira, Confluence, Slack, RSS, Notion, Linear, GitHub Discussions, Dev.to, Hacker News, Stack Overflow), tools (web-fetch, create-jira-ticket, send-slack-message, create-github-issue), and agents (researcher, incident-responder). Search by name or tag with `ctx registry search <query>`, scaffold a config entry with `ctx registry add connectors/<name>`, and copy a script into a writable registry for local edits with `ctx registry override <extension>`.

**The cheat/cheat model.**  
We didn’t invent this. [cheat/cheat](https://github.com/cheat/cheat) — the CLI cheat sheet tool — has had it for years: **multiple “cheatpaths”** (directories of markdown sheets), a **read-only community repo** as the default source, and **precedence ordering** so your local or company sheets override the community one without merge conflicts. You run `cheat -l` and get one unified list; the same path wins every time. We wanted that for Context Harness: one community repo, one `ctx registry init`, and a clear rule for who wins when the same extension exists in more than one place.

So our precedence is:

1. **Explicit `ctx.toml` entries** — always highest
2. **`.ctx/` project-local directory** — per-project overrides
3. **Personal registry** — writable, for your customizations
4. **Company registry** — shared across your team
5. **Community registry** — the official [parallax-labs/ctx-registry](https://github.com/parallax-labs/ctx-registry) repo

Higher precedence always wins. You never merge upstream community changes into your overrides; you pull the community repo when you want updates and your overrides stay in a different path. That’s the same idea as cheat’s cheatpaths, and it scales: more registries (company, team, personal) just slot into the order. We’ve documented it in the [registry section](/docs/registry/overview/) and called out the design debt to cheat there too.

**Discovery.**  
Tools and agents from registries are **auto-discovered** by the MCP server — once the registry is installed, they show up in `ctx tool list` and `ctx agent list` and are callable via MCP. Connectors need explicit activation (they need credentials), so you use `ctx registry add connectors/<name>` to get the config stub and fill in your API keys or tokens.

---

### Local embeddings: now on every binary

We’ve had local embeddings for a while (fastembed, optional tract path). What changed is **coverage**: every pre-built binary we ship now includes a local embedding provider — no separate “with embeddings” build. Primary platforms use [fastembed](https://github.com/nicelgueta/fastembed-rs) with a bundled ONNX Runtime (rustls, no system OpenSSL); Linux musl and macOS Intel use the pure-Rust [tract](https://github.com/sonos/tract) backend. Same config, same `provider = "local"`; the binary picks the right backend.

| Binary | Local embeddings | OpenAI / Ollama |
|--------|------------------|------------------|
| Linux x86_64 (glibc) | ✅ fastembed | ✅ |
| Linux x86_64 (musl) | ✅ tract | ✅ |
| Linux aarch64 | ✅ fastembed | ✅ |
| macOS x86_64 (Intel) | ✅ tract | ✅ |
| macOS aarch64 (Apple Silicon) | ✅ fastembed | ✅ |
| Windows x86_64 | ✅ fastembed | ✅ |

So you can run hybrid search with no API keys: set `provider = "local"` in `[embedding]`, run `ctx embed pending`, and use `ctx search "…" --mode hybrid`. Models download on first use and then it’s fully offline. We documented the platform table and build features in the [configuration reference](/docs/reference/configuration/).

---

### Ollama and OpenAI

You still have **Ollama** (local server) and **OpenAI** (cloud) as embedding options. Three providers: **local** (built-in), **ollama** (your Ollama instance), **openai** (API key). Same `ctx embed pending` / `ctx search --mode hybrid` workflow for all.

---

### Docs and discoverability

We added a full [extension registry section](/docs/registry/) to the docs (overview, usage guide, available extensions) and improved SEO and social metadata on the site. The config reference includes the embedding platform table and Nix/build notes.

---

### The build story (and Nix)

v0.4.0 and v0.4.1 never shipped: CI failed on Linux aarch64 (Zig cross-link failure) and we had leftover OpenSSL/Cross.toml assumptions. We fixed it: **Linux aarch64** builds natively on GitHub’s ARM runner (`ubuntu-24.04-arm`); **Linux musl** uses Zig + rustls; **cargo-zigbuild** is installed with `--force` in CI so cache restores don’t fail. So the binaries you download from [Releases](https://github.com/parallax-labs/context-harness/releases) are the same stack we test.

**Nix (NixOS / nix-darwin):** You can install Context Harness straight from the repo’s flake — no release tarball required. From a clone of the repo, run:

```bash
# Full binary with local embeddings (default)
nix build .#default
./result/bin/ctx --version

# Or install into your user profile (on $PATH)
nix profile install .#default
```

Without cloning, use the flake URL:

```bash
nix profile install github:parallax-labs/context-harness#default
```

The flake exposes two packages: **`.#default`** (same as `.#with-embeddings`) includes local embeddings; **`.#no-local-embeddings`** is a minimal binary if you only use OpenAI or Ollama for embeddings. For a development shell with Rust and Zig: `nix develop`. To **include Context Harness in your own Nix flake** (e.g. NixOS or Home Manager), see the [Nix flake guide](/docs/getting-started/nix-flake/) in the docs.

---

### Upgrading

```bash
# Pre-built binary (recommended)
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/

# From source
cargo install --path . --force
```

No breaking config changes. After upgrading, run `ctx registry init` if you haven’t already, and optionally set `provider = "local"` or `provider = "ollama"` and run `ctx embed pending`.

---

### What’s next

The registry is the foundation we wanted: **one command, a growing catalog, and a cheat-style model that scales** as we add more community and company registries. We’ll keep adding connectors, tools, and agents to the community repo and dogfood Context Harness on real workflows. If you’ve been waiting for a release where the extension registry is front and center and every binary has local embeddings, v0.4.2 is it. Enjoy — and thanks again to [cheat/cheat](https://github.com/cheat/cheat) for the design inspiration.
