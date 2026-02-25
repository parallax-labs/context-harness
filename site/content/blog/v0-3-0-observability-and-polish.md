+++
title = "v0.3.0: See What Your Index Is Doing"
description = "New stats, search explain, native JSON export, shell completions, and Docker support — everything you need to actually use Context Harness."
date = 2026-02-25

[taxonomies]
tags = ["release"]
+++

Context Harness v0.3.0 closes the gap between "I installed it" and "I understand what it's doing." Four new CLI commands, Docker support, and scoring transparency that makes tuning possible instead of guesswork.

### `ctx stats` — what's in my database?

After syncing, the first question is always "did it work?" Now you can answer it:

```
$ ctx stats
Context Harness — Database Stats
================================

  Database:    ./data/ctx.sqlite
  Size:        14.2 MB

  Documents:   216
  Chunks:      1386
  Embedded:    1386 / 1386 (100%)

  By source:
  SOURCE                     DOCS   CHUNKS   EMBEDDED   LAST SYNC
  ----------------------------------------------------------------------------
  git:platform                 89      412        412   3 hours ago
  filesystem:docs             127      584        584   1 day ago
  script:jira                  0        0          0   never
```

Document counts, chunk counts, embedding coverage percentage, per-source breakdown, and last sync time. One command, instant confidence.

### `ctx search --explain` — why did this rank here?

Search results show a single score. When a result feels wrong, you couldn't tell if the problem was keyword matching, semantic similarity, or the alpha weighting between them. Now you can:

```
$ ctx search "deployment process" --mode hybrid --explain
Search: mode=hybrid, alpha=0.60, candidates: 42 keyword + 80 vector

1. [0.87] git:platform / deployment-guide.md
    scoring: keyword=0.712  semantic=0.981  → hybrid=0.873
    ...

2. [0.64] filesystem:docs / deploy-notes.md
    scoring: keyword=0.890  semantic=0.471  → hybrid=0.639
    ...
```

Each result shows the normalized keyword score, semantic score, the alpha weight, and how they combined into the final hybrid score. You can see exactly what's happening and adjust `hybrid_alpha` based on data instead of guessing.

### `ctx export` — native JSON export

The static site search story (`ctx-search.js`) previously required a Python one-liner to extract `data.json` from SQLite. That's now a first-class command:

```bash
# To a file
$ ctx export --output site/static/docs/data.json
Exported 216 documents, 1386 chunks to site/static/docs/data.json

# To stdout for piping
$ ctx export | gzip > data.json.gz
```

No Python dependency, same JSON schema, works in CI pipelines.

### `ctx completions` — tab completion for your shell

```bash
# Bash
$ ctx completions bash > ~/.local/share/bash-completion/completions/ctx

# Zsh
$ ctx completions zsh > ~/.zfunc/_ctx

# Fish
$ ctx completions fish > ~/.config/fish/completions/ctx.fish
```

Covers all commands, subcommands, and flags.

### Docker support

A `Dockerfile` and `docker-compose.yml` are now included in the repo. The Dockerfile is a multi-stage build that produces a minimal Debian image with the `ctx` binary, your config, connectors, tools, and agents baked in:

```bash
$ docker compose up -d
$ curl localhost:7331/health
{"status":"ok","version":"0.3.0"}
```

See the [deployment guide](/docs/reference/deployment/) for the full Docker setup including persistent volumes, health checks, and environment variable configuration.

### Upgrading

```bash
# From source
cargo install --path . --force

# Or download pre-built binary
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
```

No breaking changes. All existing configs, databases, and MCP integrations continue to work.

### What's next

The focus shifts to dogfooding — using Context Harness on real repos and real questions to find where retrieval quality falls short. The `--explain` flag is the tool for that. Expect tuning improvements, built-in Lua connectors for common sources, and possibly a reranker if the data justifies it.
