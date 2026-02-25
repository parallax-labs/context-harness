+++
title = "Installation"
description = "Install the ctx binary from source or Docker in under a minute."
weight = 1
+++

### From source (recommended)

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
