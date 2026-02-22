+++
title = "Multi-Repo Context"
description = "Index multiple repositories and data sources into a single searchable knowledge base."
weight = 2
+++

A common pattern is indexing multiple repositories, wikis, and data sources into a single Context Harness instance. This gives your AI agents unified search across your entire engineering organization's knowledge.

### The idea

Configure multiple named Git connectors, filesystem mounts, S3 buckets, and Lua scripts — all feeding into the same SQLite database. When an agent searches, it gets results from *all* sources ranked together.

```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│ git:platform │  │  git:infra   │  │ s3:runbooks  │  │ script:jira  │
└──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘
       │                 │                 │                 │
       └─────────────────┴─────────────────┴─────────────────┘
                                   │
                          ┌────────▼────────┐
                          │  SQLite (single  │
                          │   database)      │
                          └────────┬────────┘
                                   │
                          ┌────────▼────────┐
                          │  ctx serve mcp  │
                          │  :7331          │
                          └─────────────────┘
```

### Complete multi-repo config

Here's a real-world config indexing an engineering org's docs, multiple services, and external knowledge sources:

```toml
# config/ctx.toml — Multi-repo engineering context

[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64

[retrieval]
final_limit = 15
hybrid_alpha = 0.6
candidate_k_keyword = 100
candidate_k_vector = 100

[server]
bind = "127.0.0.1:7331"

# ── Local project docs ──────────────────────────────────────

[connectors.filesystem.local]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt"]

# ── Main platform service ───────────────────────────────────

[connectors.git.platform]
url = "https://github.com/acme/platform.git"
branch = "main"
root = "."
include_globs = [
    "docs/**/*.md",
    "src/**/*.rs",
    "README.md",
    "CHANGELOG.md",
    "ADR/**/*.md",
]
exclude_globs = ["**/target/**", "**/node_modules/**"]
shallow = true
cache_dir = "./data/.git-cache/platform"

# ── Additional Git repos ────────────────────────────────────

[connectors.git.infra]
url = "https://github.com/acme/infrastructure.git"
branch = "main"
root = "docs/"
include_globs = ["**/*.md"]
shallow = true

[connectors.git.auth-service]
url = "https://github.com/acme/auth-service.git"
branch = "main"
include_globs = ["src/**/*.rs", "docs/**/*.md", "README.md"]
shallow = true

[connectors.git.payments]
url = "https://github.com/acme/payments.git"
branch = "main"
include_globs = ["src/**/*.rs", "docs/**/*.md"]
shallow = true

# ── Runbooks from S3 ────────────────────────────────────────

[connectors.s3.runbooks]
bucket = "acme-engineering"
prefix = "runbooks/"
region = "us-east-1"
include_globs = ["**/*.md"]

# ── Jira issues ─────────────────────────────────────────────

[connectors.script.jira]
path = "connectors/jira.lua"
timeout = 60
url = "https://acme.atlassian.net"
project = "PLATFORM"
api_token = "${JIRA_API_TOKEN}"
```

### Syncing all sources

Sync everything in one command — connectors run in parallel:

```bash
$ ctx sync all
Syncing 7 connector instances (parallel scan)...
sync filesystem:local
  fetched: 47 items
  upserted documents: 47
  chunks written: 203
ok
sync git:auth-service
  fetched: 24 items
  upserted documents: 24
  chunks written: 112
ok
sync git:infra
  fetched: 31 items
  upserted documents: 31
  chunks written: 89
ok
sync git:payments
  fetched: 18 items
  upserted documents: 18
  chunks written: 67
ok
sync git:platform
  fetched: 89 items
  upserted documents: 89
  chunks written: 412
ok
sync s3:runbooks
  fetched: 34 items
  upserted documents: 34
  chunks written: 156
ok
sync script:jira
  fetched: 142 items
  upserted documents: 142
  chunks written: 284
ok
```

Or sync specific types or instances:

```bash
$ ctx sync git               # All git connectors (parallel)
$ ctx sync git:platform      # Just one repo
$ ctx sync s3                # All S3 connectors
$ ctx sync script:jira       # One Lua connector
```

### Filtering by source

When searching, you can filter results to a specific source:

```bash
# Search only the platform repo
$ ctx search "auth middleware" --source git:platform

# Search only Jira issues
$ ctx search "payment timeout" --source script:jira

# Search everything (default)
$ ctx search "deployment procedure"
```

Via the API:

```bash
$ curl -s localhost:7331/tools/search \
    -d '{"query": "error handling", "source": "git:auth-service"}' | jq .
```

### Cursor workspace with multi-repo context

If you work in a Cursor workspace with multiple repos, a single Context Harness instance can provide unified context across all of them.

**Setup:**

1. Create a shared config directory:

```
~/ctx-workspace/
├── config/
│   └── ctx.toml          # Multi-repo config (as above)
├── data/
│   └── ctx.sqlite         # Shared database
├── connectors/
│   └── jira.lua           # Lua connector for Jira
└── scripts/
    └── sync-all.sh        # Sync script
```

2. Add to every Cursor workspace — create `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "org-context": {
      "url": "http://localhost:7331"
    }
  }
}
```

3. Start the server once and use it everywhere:

```bash
$ cd ~/ctx-workspace
$ ctx serve mcp --config ./config/ctx.toml
```

Now every Cursor window in every repo has access to the full org knowledge base.

### CI-based multi-repo index

For teams, build the index in CI so everyone gets fresh context:

```yaml
# .github/workflows/build-context.yml
name: Build Context Index
on:
  schedule:
    - cron: '0 */6 * * *'  # Every 6 hours
  workflow_dispatch:

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --git https://github.com/parallax-labs/context-harness

      - name: Sync all sources
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
          JIRA_API_TOKEN: ${{ secrets.JIRA_API_TOKEN }}
          GITHUB_TOKEN: ${{ secrets.GH_PAT }}
        run: |
          ctx init --config ./config/ctx.toml
          ctx sync all --full --config ./config/ctx.toml
          ctx embed pending --config ./config/ctx.toml

      - name: Upload database
        uses: actions/upload-artifact@v4
        with:
          name: ctx-database
          path: data/ctx.sqlite
```

Team members download the latest `ctx.sqlite` and run `ctx serve mcp` locally — instant multi-repo context without needing API keys or waiting for syncs.
