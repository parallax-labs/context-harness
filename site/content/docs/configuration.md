+++
title = "Configuration"
description = "Complete reference for the ctx.toml configuration file."
weight = 3

[extra]
sidebar_label = "Config File"
sidebar_group = "Configuration"
sidebar_order = 3
+++

Context Harness uses a TOML config file passed via `--config`. See [`ctx.example.toml`](https://github.com/parallax-labs/context-harness/blob/main/config/ctx.example.toml) for a complete annotated example.

## Full Reference

```toml
[db]
path = "./data/ctx.sqlite"          # SQLite database file path

[chunking]
max_tokens = 700                    # Max tokens per chunk (~4 chars/token)
overlap_tokens = 80                  # Overlap between consecutive chunks

[embedding]
provider = "disabled"                # "disabled" or "openai"
# model = "text-embedding-3-small"  # Required when provider != "disabled"
# dims = 1536                       # Embedding dimensions
# batch_size = 64                   # Texts per API call
# max_retries = 5                   # Retry count for API failures
# timeout_secs = 30                 # Per-request timeout

[retrieval]
final_limit = 12                     # Max results returned
hybrid_alpha = 0.6                   # 0.0 = keyword only, 1.0 = semantic only
candidate_k_keyword = 80             # FTS candidates to fetch
candidate_k_vector = 80              # Vector candidates to fetch
group_by = "document"                # Group results by document
doc_agg = "max"                      # Aggregation: max chunk score
max_chunks_per_doc = 3               # Max chunks per document in results

[server]
bind = "127.0.0.1:7331"             # HTTP server bind address
```

## Sections

| Section | Purpose |
|---------|---------|
| `[db]` | SQLite database file path |
| `[chunking]` | Token limits for text chunking |
| `[embedding]` | Provider, model, dimensions, batching |
| `[retrieval]` | Hybrid alpha, candidate counts, result limits |
| `[server]` | HTTP bind address |
| `[connectors.*]` | Data source configurations |
| `[connectors.script.*]` | Lua scripted connector configs |
| `[tools.script.*]` | Lua scripted tool configs |

## Environment Variable Expansion

String values in `[connectors.script.*]` and `[tools.script.*]` configs support `${VAR_NAME}` expansion:

```toml
[connectors.script.jira]
path = "connectors/jira.lua"
api_token = "${JIRA_API_TOKEN}"
```

