+++
title = "Quick Start"
description = "Go from zero to searchable knowledge base in 5 minutes."
weight = 2
+++

This guide takes you from installation to having an AI agent searching your codebase in 5 minutes.

### 1. Create a config file

```bash
$ mkdir -p config
$ cat > config/ctx.toml << 'EOF'
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

[connectors.filesystem.local]
root = "./docs"
include_globs = ["**/*.md", "**/*.rs", "**/*.txt"]
exclude_globs = ["**/target/**", "**/node_modules/**"]
EOF
```

### 2. Initialize and sync

```bash
$ ctx init
Database initialized successfully.

$ ctx sync filesystem
sync filesystem:local
  fetched: 47 items
  upserted documents: 47
  chunks written: 203
ok
```

Every matching file is now chunked and indexed in SQLite with FTS5 full-text search.

### 3. Search from the CLI

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
```

In another terminal, verify it works:

```bash
$ curl -s localhost:7331/health
{"status":"ok"}

$ curl -s localhost:7331/tools/search \
    -H "Content-Type: application/json" \
    -d '{"query": "auth"}' | jq '.results[0]'
{
  "id": "a1b2c3d4-...",
  "source": "filesystem:local",
  "title": "src/auth.rs",
  "score": 0.94,
  "snippet": "JWT signing key loaded from..."
}
```

### 5. Connect to Cursor

Create `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

Cursor connects to the `/mcp` endpoint, which speaks the MCP Streamable HTTP protocol (JSON-RPC). Now ask Cursor's agent: *"Search our docs for the deployment procedure"* — it will call Context Harness automatically.

### 6. Enable semantic search (optional)

Keyword search works great out of the box. For hybrid search that understands *meaning*, add embeddings:

```bash
$ export OPENAI_API_KEY="sk-..."
```

Add to `config/ctx.toml`:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
```

Generate embeddings:

```bash
$ ctx embed pending
Embedding 203 chunks... done (4.2s)

$ ctx search "how does the system handle failures" --mode hybrid
# Now finds conceptually related docs, not just keyword matches
```

You can also use **Ollama** (`provider = "ollama"`) or **local** ONNX models (`provider = "local"`) for fully offline embeddings. Some pre-built binaries (Linux musl, macOS Intel) do not include the local provider — see the [configuration reference](/docs/reference/configuration/) for the platform table.

### 7. Add a remote repo (optional)

Index a GitHub repo alongside your local files:

```toml
# Add to config/ctx.toml:
[connectors.git.platform]
url = "https://github.com/acme/platform.git"
branch = "main"
include_globs = ["docs/**/*.md", "src/**/*.rs"]
shallow = true
```

```bash
$ ctx sync git:platform
sync git:platform
  cloning https://github.com/acme/platform.git...
  fetched: 89 items
  upserted documents: 89
  chunks written: 412
ok

# Search now returns results from both filesystem and git sources
$ ctx search "deploy" --mode hybrid

# Or sync everything at once:
$ ctx sync all
```

### What's next?

- [Configuration](@/docs/reference/configuration.md) — full reference for `ctx.toml`
- [Connectors](@/docs/connectors/built-in.md) — filesystem, Git, and S3 setup
- [Lua Connectors](@/docs/connectors/lua-connectors.md) — index Jira, Slack, Notion, anything
- [Lua Tools](@/docs/connectors/lua-tools.md) — give AI agents custom actions
- [Agent Integration](@/docs/guides/agent-integration.md) — connect to Cursor, Claude Desktop, Continue.dev
- [Multi-Repo Context](@/docs/guides/multi-repo.md) — unified search across multiple repos
- [Build a RAG Agent](@/docs/guides/rag-agent.md) — build a Python agent with your knowledge base
- [Deployment](@/docs/reference/deployment.md) — Docker, systemd, CI/CD
