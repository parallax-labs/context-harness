+++
title = "Quick Start"
description = "Go from zero to searchable knowledge base in 60 seconds."
weight = 2

[extra]
sidebar_label = "Quick Start"
sidebar_group = "Getting Started"
sidebar_order = 2
+++

### 1. Create a config file

```toml
# config/ctx.toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700          # ~2800 chars per chunk
overlap_tokens = 80       # overlap between chunks for context

[retrieval]
final_limit = 12          # max results per query
hybrid_alpha = 0.6        # blend: 0 = keyword only, 1 = semantic only

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem]
root = "./docs"
include_globs = ["**/*.md", "**/*.rs", "**/*.txt"]
exclude_globs = ["**/target/**", "**/node_modules/**"]
```

### 2. Initialize and sync

```bash
$ ctx init
Database initialized successfully.

$ ctx sync filesystem
sync filesystem
  fetched: 47 items
  upserted documents: 47
  chunks written: 203
ok
```

Every file matching your globs is now chunked and indexed in SQLite with FTS5 full-text search.

### 3. Search

```bash
$ ctx search "authentication flow"
1. [0.94] filesystem / src/auth.rs
   "JWT signing key loaded from AWS Secrets Manager on startup..."
2. [0.81] filesystem / docs/deployment.md
   "Key rotation procedure for production auth services..."
3. [0.72] filesystem / docs/architecture.md
   "Authentication middleware intercepts requests before routing..."
```

### 4. Start the MCP server

```bash
$ ctx serve mcp
Listening on 127.0.0.1:7331

# In another terminal:
$ curl -s localhost:7331/tools/search -d '{"query":"auth"}' | jq .results[0]
{
  "id": "a1b2c3d4-...",
  "source": "filesystem",
  "title": "src/auth.rs",
  "score": 0.94,
  "snippet": "JWT signing key loaded from..."
}
```

Your knowledge base is now queryable by Cursor, Claude, or any HTTP client.

### 5. Enable semantic search (optional)

For hybrid search that understands meaning (not just keywords), add an embedding provider:

```bash
$ export OPENAI_API_KEY="sk-..."
```

```toml
# Add to config/ctx.toml:
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
```

```bash
$ ctx embed pending
Embedding 203 chunks... done (4.2s)

$ ctx search "how does the system handle failures" --mode hybrid
# Now finds conceptually related docs, not just keyword matches
```
