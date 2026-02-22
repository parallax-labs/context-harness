+++
title = "Multi-Repo Context"
description = "Index multiple repositories and data sources into a single searchable knowledge base."
weight = 2
+++

A common pattern is indexing multiple repositories, wikis, and data sources into a single Context Harness instance. This gives your AI agents unified search across your entire engineering organization's knowledge.

### The idea

Instead of one context source, configure multiple Git connectors, filesystem mounts, and Lua scripts — all feeding into the same SQLite database. When an agent searches, it gets results from *all* sources ranked together.

```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│ platform.git │  │  infra.git   │  │  wiki (S3)   │  │ Jira (Lua)   │
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

[connectors.filesystem]
root = "./docs"
include_globs = ["**/*.md", "**/*.txt"]

# ── Main platform service ───────────────────────────────────

[connectors.git]
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

# ── You can't have two [connectors.git] sections, so use ───
# ── Lua scripted connectors for additional Git repos.      ──

[connectors.script.infra]
path = "connectors/git-repo.lua"
timeout = 120
url = "https://github.com/acme/infrastructure.git"
branch = "main"
include_patterns = "docs/,runbooks/,*.md"

[connectors.script.auth-service]
path = "connectors/git-repo.lua"
timeout = 120
url = "https://github.com/acme/auth-service.git"
branch = "main"
include_patterns = "src/,docs/,README.md"

[connectors.script.payments]
path = "connectors/git-repo.lua"
timeout = 120
url = "https://github.com/acme/payments.git"
branch = "main"
include_patterns = "src/,docs/"

# ── Runbooks from S3 ────────────────────────────────────────

[connectors.s3]
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

### Multi-repo Git connector (Lua)

Since `[connectors.git]` can only be defined once, use a reusable Lua connector for additional repos:

```lua
-- connectors/git-repo.lua
-- Generic Git repo connector — reusable for multiple repos via config

connector = {
    name = "git-repo",
    version = "0.1.0",
    description = "Clone and index a Git repository",
}

function connector.scan(config)
    local items = {}
    local tmp_dir = os.tmpname() .. "-repo"
    os.execute("rm -f " .. tmp_dir)  -- tmpname creates a file

    -- Clone the repo
    log.info("Cloning " .. config.url .. "...")
    local clone_cmd = string.format(
        "git clone --depth 1 --branch %s %s %s 2>&1",
        config.branch or "main",
        config.url,
        tmp_dir
    )
    os.execute(clone_cmd)

    -- Parse include patterns
    local patterns = {}
    for pat in (config.include_patterns or ""):gmatch("[^,]+") do
        table.insert(patterns, pat:match("^%s*(.-)%s*$"))  -- trim
    end

    -- Walk the directory and read matching files
    local find_cmd = string.format("find %s -type f 2>/dev/null", tmp_dir)
    local handle = io.popen(find_cmd)
    if handle then
        for filepath in handle:lines() do
            local rel = filepath:sub(#tmp_dir + 2)  -- relative path

            -- Check against include patterns
            local included = #patterns == 0  -- include all if no patterns
            for _, pat in ipairs(patterns) do
                if rel:find(pat, 1, true) then
                    included = true
                    break
                end
            end

            if included then
                local f = io.open(filepath, "r")
                if f then
                    local content = f:read("*a")
                    f:close()

                    if #content > 0 and #content < 500000 then
                        -- Extract repo name for source_url
                        local repo_name = config.url:match("([^/]+)%.git$")
                            or config.url:match("([^/]+)$")
                        local branch = config.branch or "main"
                        local source_url = config.url:gsub("%.git$", "")
                            .. "/blob/" .. branch .. "/" .. rel

                        table.insert(items, {
                            source_id  = rel,
                            title      = rel,
                            body       = content,
                            source_url = source_url,
                            metadata   = { repo = repo_name },
                        })
                    end
                end
            end
        end
        handle:close()
    end

    -- Cleanup
    os.execute("rm -rf " .. tmp_dir)

    log.info("Indexed " .. #items .. " files from " .. config.url)
    return items
end
```

### Syncing all sources

Sync each source independently:

```bash
# Built-in connectors
$ ctx sync filesystem
$ ctx sync git
$ ctx sync s3

# Lua scripted repos
$ ctx sync script:infra
$ ctx sync script:auth-service
$ ctx sync script:payments
$ ctx sync script:jira

# Generate embeddings for everything
$ ctx embed pending
```

Or create a sync script:

```bash
#!/bin/bash
# scripts/sync-all.sh — Sync all data sources
set -euo pipefail

echo "==> Syncing all sources..."
ctx sync filesystem
ctx sync git
ctx sync s3
ctx sync script:infra
ctx sync script:auth-service
ctx sync script:payments
ctx sync script:jira

echo "==> Generating embeddings..."
ctx embed pending

echo "==> Done!"
ctx sources
```

### Filtering by source

When searching, you can filter results to a specific source:

```bash
# Search only the platform repo
$ ctx search "auth middleware" --source git

# Search only Jira issues
$ ctx search "payment timeout" --source "script:jira"

# Search everything (default)
$ ctx search "deployment procedure"
```

Via the API:

```bash
$ curl -s localhost:7331/tools/search \
    -d '{"query": "error handling", "source": "script:auth-service"}' | jq .
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
│   └── git-repo.lua       # Reusable Git connector
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
          ctx sync git --full --config ./config/ctx.toml
          ctx sync script:infra --config ./config/ctx.toml
          ctx sync script:auth-service --config ./config/ctx.toml
          ctx sync script:jira --config ./config/ctx.toml
          ctx embed pending --config ./config/ctx.toml

      - name: Upload database
        uses: actions/upload-artifact@v4
        with:
          name: ctx-database
          path: data/ctx.sqlite
```

Team members download the latest `ctx.sqlite` and run `ctx serve mcp` locally — instant multi-repo context without needing API keys or waiting for syncs.

