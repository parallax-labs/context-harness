+++
title = "Search your project documents"
description = "Index local documents, verify keyword search, and optionally connect an AI client."
weight = 2
+++

Install Context Harness before starting. This guide creates a local document
index and verifies search; it does not configure a standalone agent. To run the
project researcher, follow [Build Local Agents](@/docs/agents/build-local-agents.md).
See the [installation guide](@/docs/getting-started/installation.md) for release
binaries and other installation options.

### 1. Create a config file

Create `config/ctx.toml` from your project root. The example uses `./docs`, so
run it from a project containing that directory, or change `root` to an existing
document directory. Connector paths resolve from the command's working directory.
The initial configuration uses keyword-only retrieval and does not need a model.

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "disabled"

[retrieval]
final_limit = 12
hybrid_alpha = 0.6

[connectors.filesystem.local]
root = "./docs"
include_globs = ["**/*.md", "**/*.rs", "**/*.txt"]
exclude_globs = ["**/target/**", "**/node_modules/**"]
```

### 2. Initialize and sync

```sh
ctx --config ./config/ctx.toml init
ctx --config ./config/ctx.toml sync filesystem
```

The indexed documents and their keyword-search data are stored in the configured local SQLite database. Actual output depends on your sources and configuration.

### 3. Search from the CLI

```sh
ctx --config ./config/ctx.toml search "deployment"
```

Choose a term present in your documents. If no results appear, check the
connector root, include globs, and sync result before connecting an AI client.

### 4. Start the MCP server (optional)

Start the server from your project root with the same explicit configuration:

```sh
ctx --config ./config/ctx.toml serve mcp
```

In another terminal, you can check the server health and search endpoint. The
responses reflect the server and documents you have configured:

```sh
curl -s localhost:7331/health
curl -s localhost:7331/tools/search \
    -H "Content-Type: application/json" \
    -d '{"query": "auth"}'
```

### 5. Connect an MCP-compatible client

For example, create `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

Ask your client to search the indexed docs for the deployment procedure. Tool
use depends on the client's configuration and behavior; verify that it calls
Context Harness.

### 6. Enable semantic search (optional)

Keyword search needs no model. To add cloud embeddings, first configure API
credentials for your provider. Cloud embeddings send text to that provider.
Local embeddings may require an initial model download.

Replace the initial `[embedding]` section in `config/ctx.toml` with this OpenAI
example; do not add a second `[embedding]` table:

```toml
[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
```

Then use the embedding and search commands with the same explicit configuration:

```sh
ctx --config ./config/ctx.toml embed pending
ctx --config ./config/ctx.toml search "how does the system handle failures" --mode hybrid
```

Ollama and local embedding providers are also available. See the
[configuration reference](@/docs/reference/configuration.md) for provider and
platform details.

### 7. Add a remote repository (optional)

To index a Git repository, replace this placeholder with an accessible repository
URL before syncing it:

```toml
[connectors.git.platform]
url = "https://github.com/acme/platform.git" # Placeholder: replace with an accessible repository URL.
branch = "main"
include_globs = ["docs/**/*.md", "src/**/*.rs"]
shallow = true
```

Use the same explicit config for Git sync and any later search:

```sh
ctx --config ./config/ctx.toml sync git:platform
ctx --config ./config/ctx.toml search "deploy" --mode hybrid
ctx --config ./config/ctx.toml sync all
```

### What's next?

- [Configuration](@/docs/reference/configuration.md) — full reference for `ctx.toml`
- [Connectors](@/docs/connectors/built-in.md) — filesystem, Git, and S3 setup
- [Lua Connectors](@/docs/connectors/lua-connectors.md) — index Jira, Slack, Notion, anything
- [Lua Tools](@/docs/connectors/lua-tools.md) — give AI agents custom actions
- [Profiles Overview](@/docs/profiles/overview.md) — give an external client reusable project instructions
- [Build Local Agents](@/docs/agents/build-local-agents.md) — configure, run, and inspect the project researcher
- [Connect External AI Clients](@/docs/guides/agent-integration.md) — connect to Cursor, Claude Desktop, and Continue.dev
- [Multi-Repo Context](@/docs/guides/multi-repo.md) — index multiple repositories into one searchable knowledge base
- [Build a RAG Agent](@/docs/guides/rag-agent.md) — build a Python agent with your knowledge base
- [Deployment](@/docs/reference/deployment.md) — Docker, systemd, CI/CD
