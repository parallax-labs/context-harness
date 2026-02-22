+++
title = "Quick Start"
description = "Get Context Harness running in 60 seconds."
weight = 2

[extra]
sidebar_label = "Quick Start"
sidebar_group = "Getting Started"
sidebar_order = 2
+++

## 1. Create a Config File

```bash
$ cp config/ctx.example.toml config/ctx.toml
```

Edit `config/ctx.toml` to point at your documentation:

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700

[retrieval]
final_limit = 12

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem]
root = "./docs"
include_globs = ["**/*.md", "**/*.rs"]
```

## 2. Initialize and Sync

```bash
$ ctx init --config ./config/ctx.toml
# Database initialized successfully.

$ ctx sync filesystem --config ./config/ctx.toml
# sync filesystem
#   fetched: 47 items
#   upserted documents: 47
#   chunks written: 203
# ok
```

## 3. Search

```bash
$ ctx search "authentication" --config ./config/ctx.toml
# 1. [0.94] filesystem / auth-module.rs
#    JWT signing key loaded from AWS Secrets Manager...
# 2. [0.81] filesystem / deployment-runbook.md
#    Key rotation procedure for production services...
```

## 4. Start the MCP Server

```bash
$ ctx serve mcp --config ./config/ctx.toml
# Listening on 127.0.0.1:7331
```

Your knowledge base is now queryable by Cursor, Claude, or any HTTP client.

## 5. Enable Embeddings (Optional)

For semantic and hybrid search, configure an embedding provider:

```bash
$ export OPENAI_API_KEY="sk-..."
```

Update your config:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
```

Then generate embeddings:

```bash
$ ctx embed pending --config ./config/ctx.toml
$ ctx search "deployment" --mode hybrid --config ./config/ctx.toml
```

