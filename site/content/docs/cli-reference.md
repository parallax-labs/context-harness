+++
title = "CLI Reference"
description = "Every ctx command, flag, and option with examples."
weight = 13

[extra]
sidebar_label = "CLI Reference"
sidebar_group = "Reference"
sidebar_order = 12
+++

### Global options

```
ctx [OPTIONS] <COMMAND>

Options:
  -c, --config <PATH>  Config file path [default: ./config/ctx.toml]
  -h, --help           Show help
  -V, --version        Show version
```

All commands respect the `--config` flag. If omitted, Context Harness looks for `./config/ctx.toml` relative to the current directory.

---

### `ctx init`

Create the SQLite database and run migrations. Safe to run multiple times — it's idempotent.

```bash
$ ctx init
Database initialized successfully.

# With explicit config
$ ctx --config /etc/ctx/ctx.toml init
Database initialized successfully.
```

---

### `ctx sync <source> [--full]`

Sync a data source. Fetches items, normalizes to documents, splits into chunks. Incremental by default — only changed content is re-processed.

```bash
# Built-in connectors
$ ctx sync filesystem
sync filesystem
  fetched: 127 items
  upserted documents: 127
  chunks written: 584
ok

$ ctx sync git
sync git
  cloning https://github.com/acme/platform.git (shallow)...
  fetched: 89 items
  upserted documents: 89
  chunks written: 412
ok

$ ctx sync s3
sync s3
  listing s3://acme-docs/engineering/...
  fetched: 34 items
  upserted documents: 34
ok

# Lua scripted connectors (name matches [connectors.script.<name>])
$ ctx sync script:jira
$ ctx sync script:slack
$ ctx sync script:notion

# Force full re-sync (ignores checkpoint, re-processes everything)
$ ctx sync git --full
```

---

### `ctx search <query> [options]`

Search the indexed knowledge base. Supports keyword (BM25), semantic (vector), and hybrid modes.

```bash
# Keyword search (default, no embeddings needed)
$ ctx search "deployment procedure"
1. [0.94] git / docs/deploy.md
   "Production deployment follows the blue-green pattern..."
2. [0.87] filesystem / runbooks/deploy-checklist.md
   "Pre-deployment checklist: 1. Run integration tests..."
3. [0.72] script:jira / PLATFORM-1234
   "Deployment pipeline failing on staging..."

# Semantic search (requires embeddings)
$ ctx search "how to ship code to production" --mode semantic

# Hybrid search (best of both)
$ ctx search "auth middleware" --mode hybrid --limit 5

# Filter by source
$ ctx search "error handling" --source git
$ ctx search "sprint priorities" --source "script:jira"
```

| Flag | Default | Description |
|------|---------|-------------|
| `--mode` | `keyword` | `keyword`, `semantic`, or `hybrid` |
| `--limit` | from config | Max results to return |
| `--source` | all | Filter to a specific source name |

---

### `ctx get <id>`

Retrieve a full document by UUID. The UUID comes from search results.

```bash
$ ctx get a1b2c3d4-e5f6-7890-abcd-ef1234567890
{
  "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "source": "git",
  "source_id": "docs/deploy.md",
  "source_url": "https://github.com/acme/platform/blob/main/docs/deploy.md",
  "title": "docs/deploy.md",
  "body": "# Deployment Guide\n\nProduction deployment follows...",
  "updated_at": "2024-01-15T10:30:00Z"
}
```

---

### `ctx sources`

List all data sources and their document/chunk counts.

```bash
$ ctx sources
Source              Documents   Chunks   Status
filesystem          127         584      ok
git                 89          412      ok
s3                  34          156      ok
script:jira         234         234      ok
script:slack        0           0        not synced
```

---

### `ctx embed pending`

Generate embeddings for chunks that haven't been embedded yet. Requires `[embedding]` config.

```bash
$ ctx embed pending
Embedding 203 chunks... done (4.2s)

# No-op if everything is already embedded
$ ctx embed pending
All chunks already embedded.
```

### `ctx embed rebuild`

Drop all embeddings and regenerate from scratch. Useful after changing the embedding model or dimensions.

```bash
$ ctx embed rebuild
Dropping all embeddings...
Embedding 584 chunks... done (18.7s)
```

---

### `ctx serve mcp`

Start the MCP-compatible HTTP server. Discovers Lua tools at startup.

```bash
$ ctx serve mcp
Loaded 2 Lua tool(s):
  POST /tools/echo — Echoes back the input message
  POST /tools/create_jira_ticket — Create a Jira ticket enriched with related context
Listening on 127.0.0.1:7331
```

The server binds to `[server].bind` from config. See [MCP Server](/docs/mcp-server/) for endpoint documentation.

---

### `ctx connector init <name>`

Scaffold a new Lua connector from a commented template.

```bash
$ ctx connector init slack
Created connectors/slack.lua
```

### `ctx connector test <path> [--source <name>]`

Test a Lua connector by running it and printing the returned items *without* modifying the database.

```bash
# Test with minimal context (no config needed)
$ ctx connector test connectors/slack.lua
scan() returned 42 items:
  [0] source_id="msg-001" title="Sprint planning notes"
  [1] source_id="msg-002" title="Deployment update"
  ...

# Test with config from [connectors.script.slack]
$ ctx connector test connectors/slack.lua --source slack
```

---

### `ctx tool init <name>`

Scaffold a new Lua tool from a commented template.

```bash
$ ctx tool init post_slack
Created tools/post_slack.lua
```

### `ctx tool test <path> [--param key=value] [--source <name>]`

Test a Lua tool by executing it with sample parameters.

```bash
$ ctx tool test tools/echo.lua --param message="hello world"
Tool: echo v0.1.0
  Description: Echoes back the input message
  Parameters: message (string, required)

Result:
{
  "echo": "Echo: hello world",
  "source_count": 3
}

# Test with config from [tools.script.create_jira_ticket]
$ ctx tool test tools/create-jira-ticket.lua \
    --param title="Fix auth bug" \
    --param priority="High" \
    --source create_jira_ticket
```

### `ctx tool list`

List all registered tools (built-in + Lua) with their parameter schemas.

```bash
$ ctx tool list
Built-in tools:
  search        Search indexed documents (keyword, semantic, hybrid)
  get           Get full document content by ID
  sources       List all configured data sources

Lua tools:
  echo          Echoes back the input message
                Parameters: message (string, required)
  create_jira_ticket  Create a Jira ticket enriched with context
                Parameters: title (string, required), priority (enum, optional)
```
