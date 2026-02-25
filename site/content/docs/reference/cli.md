+++
title = "CLI Reference"
description = "Every ctx command, flag, and option with examples."
weight = 4
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

### `ctx stats`

Show database statistics — document, chunk, and embedding counts with a per-source breakdown.

```bash
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

### `ctx sync <connector> [--full]`

Sync data sources. Fetches items, normalizes to documents, splits into chunks. Incremental by default — only changed content is re-processed.

Connector format: `all`, `<type>`, or `<type>:<name>`.

```bash
# Sync everything (parallel)
$ ctx sync all
Syncing 5 connector instances (parallel scan)...
sync filesystem:docs
  fetched: 127 items
  upserted documents: 127
  chunks written: 584
ok
sync git:platform
  fetched: 89 items
  upserted documents: 89
  chunks written: 412
ok

# Sync all connectors of a type
$ ctx sync git                    # All git connectors
$ ctx sync filesystem             # All filesystem connectors

# Sync a specific named instance
$ ctx sync git:platform
$ ctx sync filesystem:docs
$ ctx sync s3:runbooks
$ ctx sync script:jira

# Force full re-sync (ignores checkpoint, re-processes everything)
$ ctx sync git:platform --full
$ ctx sync all --full
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

```bash
# Show scoring breakdown
$ ctx search "auth middleware" --mode hybrid --explain
Search: mode=hybrid, alpha=0.60, candidates: 42 keyword + 80 vector

1. [0.87] git:platform / auth-middleware.md
    scoring: keyword=0.712  semantic=0.981  → hybrid=0.873
    updated: 2026-02-20T14:32:00Z
    excerpt: "The auth middleware validates JWT tokens..."
    id: a1b2c3d4-...
```

| Flag | Default | Description |
|------|---------|-------------|
| `--mode` | `keyword` | `keyword`, `semantic`, or `hybrid` |
| `--limit` | from config | Max results to return |
| `--source` | all | Filter to a specific source name |
| `--explain` | off | Show keyword, semantic, and hybrid score breakdown |

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

The server binds to `[server].bind` from config. See [MCP Server API](@/docs/reference/mcp-server.md) for endpoint documentation.

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

---

### `ctx agent list`

List all configured agents with descriptions and tool lists.

```bash
$ ctx agent list
  code-reviewer        Reviews code changes against project conventions   (tools: search, get)        [toml]
  architect            Answers architecture questions using indexed docs   (tools: search, get, sources) [toml]
  incident-responder   Helps triage production incidents with runbooks     (tools: search, get, create_jira_ticket) [lua]
```

### `ctx agent test <name> [--arg key=value]`

Resolve an agent's prompt with arguments and print the result. Useful for debugging Lua agents.

```bash
$ ctx agent test incident-responder --arg service=payments-api --arg severity=P1

Agent: incident-responder
Source: lua (agents/incident-responder.lua)
Tools: search, get, create_jira_ticket

System prompt (487 chars):
  You are an incident responder for the payments-api service (P1 severity).
  ...

Messages (1):
  [assistant] I'm ready to help with the P1 payments-api incident...

# Test a TOML agent (no dynamic resolution)
$ ctx agent test code-reviewer

Agent: code-reviewer
Source: toml
Tools: search, get

System prompt (245 chars):
  You are a senior code reviewer for this project...
```

### `ctx agent init <name>`

Scaffold a new Lua agent script from a template.

```bash
$ ctx agent init sre-helper
Created: agents/sre-helper.lua
Add to config:

  [agents.script.sre-helper]
  path = "agents/sre-helper.lua"
  timeout = 30
```

---

### `ctx registry <command>`

Manage extension registries — community connectors, tools, and agents from Git-backed repositories.

```bash
$ ctx registry --help
Commands:
  list      List configured registries and available extensions
  install   Install (clone) configured registries
  update    Update (git pull) registries
  search    Search extensions by name, tag, or description
  info      Show details for a specific extension
  add       Scaffold a config entry for an extension in ctx.toml
  override  Copy an extension to a writable registry for customization
  init      Install the community extension registry (first-run setup)
```

#### `ctx registry init`

Clone the community registry and add it to your config. Safe to run multiple times.

```bash
$ ctx registry init
Cloning community extension registry...
Installed: 10 connectors, 4 tools, 2 agents
Added [registries.community] to ./config/ctx.toml
```

#### `ctx registry list`

Show all configured registries and their available extensions, grouped by type.

```bash
$ ctx registry list
```

#### `ctx registry search <query>`

Find extensions by name, tag, or description.

```bash
$ ctx registry search slack
Found 2 extensions matching 'slack':
  connectors/slack — Ingest message history from Slack channels
  tools/send-slack-message — Post a message to a Slack channel
```

#### `ctx registry info <extension>`

Show full details for an extension. Extension IDs use the `type/name` format.

```bash
$ ctx registry info connectors/rss
Extension: connectors/rss
Registry:  community
Script:    ~/.ctx/registries/community/connectors/rss/connector.lua
Description: Ingest articles from RSS and Atom feeds
Tags: rss, atom, feed, blog, news
Required config: feed_url
Host APIs: http
```

#### `ctx registry add <extension>`

Scaffold a `ctx.toml` config entry for a connector, pre-filled with required fields.

```bash
$ ctx registry add connectors/jira
Added [connectors.script.jira] to ./config/ctx.toml
Edit ./config/ctx.toml to set: url, email, api_token
```

#### `ctx registry install [--name <registry>]`

Clone registries that have `url` configured but aren't cloned yet.

```bash
$ ctx registry install
```

#### `ctx registry update [--name <registry>]`

Pull the latest changes for Git-backed registries.

```bash
$ ctx registry update
```

#### `ctx registry override <extension>`

Copy an extension from a read-only registry to a writable one for local customization.

```bash
$ ctx registry override connectors/rss
Copied connectors/rss to ~/.ctx/registries/personal/connectors/rss/
```

---

### `ctx export [--output <path>]`

Export the search index as JSON for use with `ctx-search.js` on static sites. Replaces the Python export script.

```bash
# Export to stdout (for piping)
$ ctx export > data.json

# Export to a file
$ ctx export --output site/static/docs/data.json
Exported 216 documents, 1386 chunks to site/static/docs/data.json
```

---

### `ctx completions <shell>`

Generate shell completion scripts for tab completion of commands, flags, and arguments.

```bash
# Bash
$ ctx completions bash > ~/.local/share/bash-completion/completions/ctx

# Zsh
$ ctx completions zsh > ~/.zfunc/_ctx

# Fish
$ ctx completions fish > ~/.config/fish/completions/ctx.fish
```
