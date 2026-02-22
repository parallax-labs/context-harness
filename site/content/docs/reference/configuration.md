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
provider = "disabled"                  # "disabled" | "openai"
# model = "text-embedding-3-small"    # OpenAI model name
# dims = 1536                         # Embedding vector dimensions
# batch_size = 64                     # Texts per API call
# max_retries = 5                     # Retry count for transient failures
# timeout_secs = 30                   # Per-request timeout

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
| `[embedding]` | Embedding provider, model, dimensions |
| `[retrieval]` | Hybrid alpha, candidate counts, result limits |
| `[server]` | HTTP bind address |
| `[connectors.filesystem.*]` | Named filesystem connector instances |
| `[connectors.git.*]` | Named git connector instances |
| `[connectors.s3.*]` | Named S3 connector instances |
| `[connectors.script.*]` | Named Lua scripted connector instances |
| `[tools.script.*]` | Lua scripted tool configs |
