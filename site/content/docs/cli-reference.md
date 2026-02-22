+++
title = "CLI Reference"
description = "Complete reference for all ctx commands."
weight = 7

[extra]
sidebar_label = "CLI Reference"
sidebar_group = "Reference"
sidebar_order = 7
+++

## Global Options

| Flag | Default | Description |
|------|---------|-------------|
| `--config <path>` | `./config/ctx.toml` | Path to configuration file |
| `-h, --help` | | Show help |
| `-V, --version` | | Show version |

## Commands

### `ctx init`

Initialize the SQLite database and run migrations.

```bash
$ ctx init --config ./config/ctx.toml
```

### `ctx sync <source>`

Sync a data source. Runs the appropriate connector, normalizes documents, and generates chunks.

```bash
$ ctx sync filesystem          # Filesystem connector
$ ctx sync git                  # Git connector
$ ctx sync s3                   # S3 connector
$ ctx sync script:my-source     # Lua scripted connector
```

| Flag | Description |
|------|-------------|
| `--full` | Force full re-sync (ignore checkpoint) |

### `ctx search <query>`

Search the indexed documents.

```bash
$ ctx search "authentication flow"
$ ctx search "deploy" --mode hybrid --limit 5
```

| Flag | Default | Description |
|------|---------|-------------|
| `--mode` | `keyword` | Search mode: `keyword`, `semantic`, or `hybrid` |
| `--limit` | from config | Maximum results |
| `--source` | all | Filter by source name |

### `ctx get <id>`

Retrieve a full document by UUID.

```bash
$ ctx get a1b2c3d4-e5f6-7890-abcd-ef1234567890
```

### `ctx sources`

List configured data sources and their status.

```bash
$ ctx sources --config ./config/ctx.toml
```

### `ctx embed pending`

Generate embeddings for chunks that don't have them yet.

```bash
$ ctx embed pending --config ./config/ctx.toml
```

### `ctx embed rebuild`

Drop all embeddings and regenerate from scratch.

```bash
$ ctx embed rebuild --config ./config/ctx.toml
```

### `ctx serve mcp`

Start the MCP-compatible HTTP server.

```bash
$ ctx serve mcp --config ./config/ctx.toml
# Listening on 127.0.0.1:7331
```

### `ctx connector init <name>`

Scaffold a new Lua scripted connector.

```bash
$ ctx connector init my-source
# Created connectors/my-source.lua
```

### `ctx connector test <path>`

Test a Lua scripted connector without modifying the database.

```bash
$ ctx connector test connectors/my-source.lua
$ ctx connector test connectors/jira.lua --source jira  # use config from [connectors.script.jira]
```

### `ctx tool init <name>`

Scaffold a new Lua tool script.

```bash
$ ctx tool init my-tool
# Created tools/my-tool.lua
```

### `ctx tool test <path>`

Test a Lua tool script with sample parameters.

```bash
$ ctx tool test tools/echo.lua --param message="hello"
```

### `ctx tool list`

List all configured tools (built-in + Lua scripts).

```bash
$ ctx tool list --config ./config/ctx.toml
```

