+++
title = "Built-in Connectors"
description = "Filesystem, Git, and S3 connectors for ingesting data from external sources."
weight = 4

[extra]
sidebar_label = "Connectors"
sidebar_group = "Configuration"
sidebar_order = 4
+++

Connectors fetch data from external sources and normalize it into a consistent `Document` model. All connectors support incremental sync via checkpoints — only changed content is re-processed on subsequent runs.

## Filesystem Connector

Scan a local directory tree for files matching glob patterns.

```toml
[connectors.filesystem]
root = "./docs"                      # Directory to scan
include_globs = ["**/*.md", "**/*.rs"]  # Files to include
exclude_globs = ["**/target/**"]       # Files to exclude
follow_symlinks = false               # Follow symbolic links
```

Each file becomes a `Document` with its content as the body, filesystem path as the source ID, and file modification time as the timestamp.

## Git Connector

Clone and index any Git repository — local or remote. Extracts per-file commit metadata (author, timestamp, SHA) and auto-generates web URLs for GitHub/GitLab repos.

```toml
[connectors.git]
url = "https://github.com/acme/platform.git"   # Repo URL or local path
branch = "main"                               # Branch to track
root = "docs/"                                 # Subdirectory to scope
include_globs = ["**/*.md", "**/*.rst"]       # File patterns
shallow = true                                  # --depth 1 clone
# cache_dir = "./data/.git-cache/platform"     # Optional clone cache
```

**Features:**
- Clones on first sync, pulls on subsequent syncs
- Per-file last commit timestamp and author from `git log`
- GitHub/GitLab web URLs auto-generated for each file
- Shallow clone support to minimize disk usage
- Incremental sync via checkpoint timestamps

## S3 Connector

Index documents from Amazon S3 buckets (or S3-compatible services like MinIO and LocalStack).

```toml
[connectors.s3]
bucket = "acme-docs"                          # Bucket name
prefix = "engineering/runbooks/"                # Key prefix filter
region = "us-east-1"                           # AWS region
include_globs = ["**/*.md", "**/*.json"]       # Key patterns
# endpoint_url = "http://localhost:9000"       # For MinIO/LocalStack
```

Requires `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` environment variables.

**Features:**
- Pagination for large buckets (1000+ objects)
- `LastModified` and `ETag` tracking for incremental sync
- Custom endpoint URL for S3-compatible services
- Glob-based include/exclude filtering on object keys

## Embedding Configuration

Embeddings enable semantic and hybrid search. Without embeddings, only keyword search (FTS5/BM25) is available.

```toml
[embedding]
provider = "openai"                  # "disabled" or "openai"
model = "text-embedding-3-small"      # OpenAI model name
dims = 1536                           # Vector dimensions
batch_size = 64                       # Texts per API call
max_retries = 5                       # Retry on failure
timeout_secs = 30                     # Per-request timeout
```

Set the `OPENAI_API_KEY` environment variable before using embedding commands. Embedding is non-fatal during sync — documents are always ingested even if embedding fails.

