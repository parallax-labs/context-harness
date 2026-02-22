+++
title = "Installation"
description = "Install the ctx binary from source in under a minute."
weight = 1

[extra]
sidebar_label = "Installation"
sidebar_group = "Getting Started"
sidebar_order = 1
+++

### From source (recommended)

```bash
$ cargo install --git https://github.com/parallax-labs/context-harness
```

Or clone and build:

```bash
$ git clone https://github.com/parallax-labs/context-harness.git
$ cd context-harness
$ cargo build --release
$ ./target/release/ctx --help
```

### Prerequisites

| Tool | Version | What for |
|------|---------|----------|
| **Rust** | 1.75+ stable | Building the `ctx` binary |
| **Git** | 2.x | Git connector (cloning repos) |

SQLite is bundled — there's nothing else to install. The binary is self-contained.

### Verify

```bash
$ ctx --version
context-harness 0.1.0

$ ctx --help
A local-first context engine for AI tools

Usage: ctx [OPTIONS] <COMMAND>
...
```
