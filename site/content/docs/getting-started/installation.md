+++
title = "Installation"
description = "Install the ctx binary from source, Nix, or Docker in under a minute."
weight = 1
+++

### Pre-built binaries (recommended)

Download the latest release for your platform from [GitHub Releases](https://github.com/parallax-labs/context-harness/releases/latest). Linux (glibc, musl, aarch64), macOS (Intel and Apple Silicon), and Windows are supported. All binaries include the local embedding provider. See [configuration](/docs/reference/configuration/) for the platform table.

### Nix (NixOS / nix-darwin)

You can install Context Harness **straight from the repo flake** — no release tarball required.

**From a clone of the repo:**

```bash
# Build the default package (full binary with local embeddings)
nix build .#default
./result/bin/ctx --version

# Or install into your user profile (on $PATH)
nix profile install .#default
```

**Without cloning (flake reference):**

```bash
nix profile install github:parallax-labs/context-harness#default
```

| Package | Description |
|---------|-------------|
| `.#default` | Full build with local embeddings (fastembed; models download on first use). |
| `.#no-local-embeddings` | Minimal binary, no local embeddings. Use with OpenAI or Ollama only. |

Use `nix develop` for a development shell with Rust and git. To **include Context Harness as a dependency in your own Nix flake** (e.g. NixOS module or Home Manager), see the [Nix flake guide](/docs/getting-started/nix-flake/).

### From source

One command:

```bash
$ cargo install --git https://github.com/parallax-labs/context-harness
```

Or clone and build:

```bash
$ git clone https://github.com/parallax-labs/context-harness.git
$ cd context-harness
$ cargo build --release
$ cp target/release/ctx ~/.local/bin/  # or anywhere on $PATH
```

### Using Docker

```bash
$ docker build -t context-harness .
$ docker run -it context-harness --help
```

Or use Docker for the server only:

```bash
$ docker run -d -p 7331:7331 \
    -v $(pwd)/config:/app/config \
    -v ctx-data:/app/data \
    -e OPENAI_API_KEY=$OPENAI_API_KEY \
    context-harness
```

### Prerequisites

| Tool | Version | What for |
|------|---------|----------|
| **Rust** | 1.75+ stable | Building the `ctx` binary |
| **Git** | 2.x | Git connector (cloning repos) |
| **Python 3** | 3.8+ | `data.json` export for static search (optional) |

SQLite is bundled via `rusqlite` — there is nothing else to install. The binary is fully self-contained with no runtime dependencies.

**Building from source:** The **local** embedding provider has no system dependencies at runtime; models are downloaded on first use.

- **Linux:** If you use default features (fastembed), install OpenSSL development headers: `libssl-dev` and `pkg-config` on Debian/Ubuntu, or `openssl-devel` on Fedora/RHEL.
- **macOS:** The build links against the C++ standard library. If you see `library not found for -lc++`, run `xcode-select --install` to install the Xcode Command Line Tools. If you use Nix, run `nix develop` first so the dev shell provides Zig as the C/C++ compiler; then `cargo build` works.

See the [configuration reference](https://parallax-labs.github.io/context-harness/docs/reference/configuration/#requirements-and-platform-support-for-local-embeddings) for details.

### Verify

```bash
$ ctx --version
context-harness 0.1.0

$ ctx --help
A local-first context engine for AI tools

Usage: ctx [OPTIONS] <COMMAND>

Commands:
  init         Initialize the database
  stats        Show database statistics
  sync         Sync a data source (filesystem, git, s3, script:<name>)
  search       Search the knowledge base
  get          Get a document by ID
  sources      List configured sources and stats
  embed        Generate or rebuild embeddings
  export       Export index as JSON for static site search
  serve        Start the HTTP/MCP server
  connector    Manage Lua connectors (init, test)
  tool         Manage Lua tools (init, test, list)
  agent        Manage agents (list, test, init)
  completions  Generate shell completions
  help         Print help

Options:
  -c, --config <PATH>  Config file [default: ./config/ctx.toml]
  -h, --help           Print help
  -V, --version        Print version
```

### Shell completion (optional)

Generate completions for your shell:

```bash
# Bash
$ ctx completions bash > ~/.local/share/bash-completion/completions/ctx

# Zsh
$ ctx completions zsh > ~/.zfunc/_ctx

# Fish
$ ctx completions fish > ~/.config/fish/completions/ctx.fish
```
