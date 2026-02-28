+++
title = "Local-first context for Cursor and Claude with Context Harness"
description = "One place to index your docs, Git repos, and runbooks—query it locally and expose it to Cursor and Claude via MCP."
date = 2026-02-26

[taxonomies]
tags = ["mcp", "cursor", "claude", "local-ai"]
+++

If you use Cursor or Claude with MCP (Model Context Protocol), you can give the AI access to your docs, Git repos, and runbooks. The catch: many setups send that context through third-party APIs or only work with a single source. I wanted one place to index everything and query it locally—no cloud dependency for the data itself.

**Context Harness** is a small Rust CLI and HTTP server that does exactly that. You point it at folders, Git repos, or S3 prefixes; it ingests into a local SQLite database, optionally embeds with a local model (or Ollama/OpenAI), and exposes search and retrieval over MCP. Cursor and Claude connect to it as an MCP server and get `search` and `get` tools over your indexed context.

## Why local-first?

- **Privacy:** Docs and code never leave your machine unless you choose an external embedding API.
- **Offline:** With local embeddings (fastembed or tract), you can index and search without any network after the first model download.
- **One config:** Same TOML and connectors whether you use local, Ollama, or OpenAI for embeddings.

## What you get

- **Connectors:** Filesystem, Git, S3, and Lua scripts for custom sources (e.g. Jira, GitHub Issues).
- **Storage:** SQLite with FTS5 for keyword search; optional vector embeddings for semantic (or hybrid) search.
- **MCP server:** One HTTP endpoint; add it to Cursor's MCP config and the AI can search and retrieve from your knowledge base.
- **CLI:** `ctx init`, `ctx sync all`, `ctx search "query"`, `ctx serve mcp`.

## Quick start

1. Download the latest [release](https://github.com/parallax-labs/context-harness/releases) for your platform (macOS, Linux, Windows).
2. Copy the example config and add a connector (e.g. a Git repo or a folder of markdown).
3. Run `ctx init`, `ctx sync all`, then `ctx serve mcp`.
4. In Cursor, add the server URL to `.cursor/mcp.json` and restart.

The [docs](https://parallax-labs.github.io/context-harness/) have a full quick start, configuration reference, and guides for Cursor, RAG, and multi-repo setups.

## Local embeddings on every platform

Recent releases ship **local** embeddings on all six targets (Linux glibc/musl, macOS Intel/Apple Silicon, Windows). No ONNX Runtime install—primary platforms use fastembed with bundled ORT; musl and Intel Mac use a pure-Rust tract backend. Same `provider = "local"` in config; models download on first use.

If you're already using MCP with Cursor or Claude and have a pile of internal docs or code, Context Harness might be worth a try. It's AGPL-3.0 licensed and the repo has a [demo](https://parallax-labs.github.io/context-harness/demo/) you can click through without installing anything.

**Repo:** [github.com/parallax-labs/context-harness](https://github.com/parallax-labs/context-harness)  
**Docs:** [parallax-labs.github.io/context-harness](https://parallax-labs.github.io/context-harness/)
