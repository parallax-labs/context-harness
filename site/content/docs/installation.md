+++
title = "Installation"
description = "Install Context Harness from source or with cargo install."
weight = 1

[extra]
sidebar_label = "Installation"
sidebar_group = "Getting Started"
sidebar_order = 1
+++

## From Source (recommended)

```bash
$ cargo install --path .

# Or directly from the repository
$ cargo install --git https://github.com/parallax-labs/context-harness
```

## Build Manually

```bash
$ git clone https://github.com/parallax-labs/context-harness.git
$ cd context-harness
$ cargo build --release
$ ./target/release/ctx --help
```

## Prerequisites

| Tool | Version | Required For |
|------|---------|-------------|
| **Rust** | 1.75+ (stable) | Building the `ctx` binary |
| **Git** | 2.x | Git connector |

SQLite is bundled — no system install needed.

## Verify Installation

```bash
$ ctx --version
# context-harness 0.1.0
```

