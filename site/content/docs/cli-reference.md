+++
title = "CLI Reference"
description = "Every ctx command, flag, and option."
weight = 7

[extra]
sidebar_label = "CLI Reference"
sidebar_group = "Reference"
sidebar_order = 7
+++

### Global options

```
ctx [OPTIONS] <COMMAND>

Options:
  --config <path>    Config file path [default: ./config/ctx.toml]
  -h, --help         Show help
  -V, --version      Show version
```

### `ctx init`

Create the SQLite database and run migrations.

```bash
$ ctx init
Database initialized successfully.
```

### `ctx sync <source> [--full]`

Sync a data source. Fetches items, normalizes to documents, generates chunks.

```bash
$ ctx sync filesystem          # Local files
$ ctx sync git                  # Git repository
$ ctx sync git --full           # Force full re-sync (ignore checkpoint)
$ ctx sync s3                   # S3 bucket
$ ctx sync script:jira          # Lua scripted connector
```

### `ctx search <query> [options]`

Search the indexed documents.

```bash
$ ctx search "deploy procedure"
$ ctx search "auth" --mode hybrid --limit 5
$ ctx search "error handling" --source git
```

| Flag | Default | Description |
|------|---------|-------------|
| `--mode` | `keyword` | `keyword`, `semantic`, or `hybrid` |
| `--limit` | from config | Max results |
| `--source` | all | Filter by source name |

### `ctx get <id>`

Retrieve a full document by UUID.

```bash
$ ctx get a1b2c3d4-e5f6-7890-abcd-ef1234567890
```

### `ctx sources`

List configured data sources and their sync status.

```bash
$ ctx sources
filesystem   configured: true   healthy: true   docs: 127
git          configured: true   healthy: true   docs: 89
s3           configured: false
```

### `ctx embed pending`

Generate embeddings for chunks that don't have them yet.

```bash
$ ctx embed pending
Embedding 203 chunks... done (4.2s)
```

### `ctx embed rebuild`

Drop all embeddings and regenerate from scratch.

### `ctx serve mcp`

Start the MCP-compatible HTTP server.

```bash
$ ctx serve mcp
Loaded 2 Lua tool(s):
  POST /tools/echo — Echoes back the input message
  POST /tools/create_jira_ticket — Create a Jira ticket enriched with related context
Listening on 127.0.0.1:7331
```

### `ctx connector init <name>`

Scaffold a new Lua connector from a template.

```bash
$ ctx connector init slack
Created connectors/slack.lua
```

### `ctx connector test <path> [--source <name>]`

Test a Lua connector without modifying the database.

```bash
$ ctx connector test connectors/slack.lua
$ ctx connector test connectors/jira.lua --source jira  # Use config from [connectors.script.jira]
```

### `ctx tool init <name>`

Scaffold a new Lua tool from a template.

### `ctx tool test <path> [--param key=value]`

Test a Lua tool with sample parameters.

```bash
$ ctx tool test tools/echo.lua --param message="hello"
```

### `ctx tool list`

List all configured tools (built-in + Lua scripts).
