+++
title = "Configuration"
description = "Complete reference for every setting in ctx.toml."
weight = 1
+++

Context Harness uses a TOML config file passed via `--config` (defaults to `./config/ctx.toml`). Here's a fully annotated example:

### Full reference

```toml
[db]
path = "./data/ctx.sqlite"            # SQLite database file path

[chunking]
max_tokens = 700                      # Max tokens per chunk (~4 chars/token)
overlap_tokens = 80                    # Overlap between consecutive chunks

[embedding]
provider = "disabled"                  # "disabled" | "openai" | "ollama" | "local"
# model = "text-embedding-3-small"    # Model name (required for openai/ollama)
# dims = 1536                         # Vector dimensions (required for openai/ollama)
# batch_size = 64                     # Texts per batch
# max_retries = 5                     # Retry count for transient failures
# timeout_secs = 30                   # Per-request timeout
# url = "http://localhost:11434"      # Ollama API base URL (ollama provider only)

#### Platform support for local embeddings

Pre-built release binaries are built for six targets. The **local** provider (fastembed/ONNX) is included in most; two targets ship without it:

| Binary | Local embeddings | OpenAI / Ollama |
|--------|------------------|------------------|
| Linux x86_64 (glibc) | ✅ | ✅ |
| Linux x86_64 (musl) | ❌ | ✅ |
| Linux aarch64 | ✅ | ✅ |
| macOS x86_64 (Intel) | ❌ | ✅ |
| macOS aarch64 (Apple Silicon) | ✅ | ✅ |
| Windows x86_64 | ✅ | ✅ |

- **Linux musl**: ONNX Runtime does not support musl; the static binary is built without the local-embeddings feature.
- **macOS Intel**: The `ort` crate does not provide prebuilt ONNX Runtime binaries for `x86_64-apple-darwin`.

On **musl** or **Intel Mac**, to use local (fully offline) embeddings you can either build from source (`cargo install --git https://github.com/parallax-labs/context-harness.git`) or use the **Ollama** provider with the prebuilt binary.

[retrieval]
final_limit = 12                       # Max results returned to caller
hybrid_alpha = 0.6                     # 0.0 = keyword only, 1.0 = semantic only
candidate_k_keyword = 80               # FTS5 candidates to fetch before re-ranking
candidate_k_vector = 80                # Vector candidates to fetch before re-ranking
group_by = "document"                  # Group chunks by parent document
doc_agg = "max"                        # Aggregation: use max chunk score
max_chunks_per_doc = 3                 # Max chunks per document in results

[server]
bind = "127.0.0.1:7331"               # HTTP server bind address

# ── Connectors (all types are named instances) ───────────

[connectors.filesystem.local]
root = "./docs"
include_globs = ["**/*.md", "**/*.rs"]
exclude_globs = ["**/target/**"]

[connectors.git.platform]
url = "https://github.com/acme/platform.git"
branch = "main"
root = "docs/"
include_globs = ["**/*.md"]
shallow = true

[connectors.git.auth-service]
url = "https://github.com/acme/auth-service.git"
branch = "main"

[connectors.s3.runbooks]
bucket = "acme-docs"
prefix = "engineering/"
region = "us-east-1"

# ── Lua scripted connectors ───────────────────────────────

[connectors.script.jira]
path = "connectors/jira.lua"
timeout = 30
url = "https://mycompany.atlassian.net"
project = "ENG"
api_token = "${JIRA_API_TOKEN}"        # ${VAR} expands env vars

# ── Lua scripted tools ────────────────────────────────────

[tools.script.create_jira_ticket]
path = "tools/create-jira-ticket.lua"
timeout = 30
jira_url = "https://mycompany.atlassian.net"
jira_project = "ENG"
jira_token = "${JIRA_API_TOKEN}"

# ── Inline agents (static prompts) ──────────────

[agents.inline.code-reviewer]
description = "Reviews code changes against project conventions"
tools = ["search", "get"]
system_prompt = """
You are a senior code reviewer. Use search to find coding conventions.
Be specific — cite which convention a suggestion relates to.
"""

[agents.inline.architect]
description = "Answers architecture questions using indexed docs"
tools = ["search", "get", "sources"]
system_prompt = """
You are a software architect. Search for ADRs and design documents.
When recommending changes, explain tradeoffs and cite sources.
"""

# ── Lua scripted agents (dynamic prompts) ────────

[agents.script.incident-responder]
path = "agents/incident-responder.lua"
timeout = 30
search_limit = 5

# ── Extension registries ─────────────────────────────────

[registries.community]
url = "https://github.com/parallax-labs/ctx-registry.git"
branch = "main"
path = "~/.ctx/registries/community"
readonly = true                        # Don't write to this registry
auto_update = true                     # Pull on startup
```

### Environment variable expansion

String values in `[connectors.script.*]` and `[tools.script.*]` configs support `${VAR_NAME}` expansion. This keeps secrets out of your config file:

```toml
[connectors.script.slack]
path = "connectors/slack.lua"
token = "${SLACK_BOT_TOKEN}"           # Expanded at runtime
workspace = "acme"                      # Plain string, no expansion
```

### Section reference

| Section | Purpose |
|---------|---------|
| `[db]` | SQLite database path |
| `[chunking]` | Token limits for text chunking |
| `[embedding]` | Embedding provider (`disabled`, `openai`, `ollama`, `local`) |
| `[retrieval]` | Hybrid alpha, candidate counts, result limits |
| `[server]` | HTTP bind address |
| `[connectors.filesystem.*]` | Named filesystem connector instances |
| `[connectors.git.*]` | Named git connector instances |
| `[connectors.s3.*]` | Named S3 connector instances |
| `[connectors.script.*]` | Named Lua scripted connector instances |
| `[tools.script.*]` | Lua scripted tool configs |
| `[agents.inline.*]` | Inline TOML agents (static system prompt) |
| `[agents.script.*]` | Lua scripted agents (dynamic prompts) |
| `[registries.*]` | Named extension registry instances |
