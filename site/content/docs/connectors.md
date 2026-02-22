+++
title = "Built-in Connectors"
description = "Filesystem, Git, and S3 connectors for ingesting data from any source."
weight = 4

[extra]
sidebar_label = "Connectors"
sidebar_group = "Configuration"
sidebar_order = 4
+++

Connectors fetch data from external sources and normalize it into a consistent Document model. All connectors support incremental sync — only changed content is re-processed on subsequent runs.

### Filesystem Connector

Scans a local directory tree. Each file becomes a Document with its content as the body.

```toml
[connectors.filesystem]
root = "./docs"                        # Directory to scan
include_globs = ["**/*.md", "**/*.rs"] # Glob patterns to include
exclude_globs = ["**/target/**"]       # Glob patterns to exclude
follow_symlinks = false                # Follow symbolic links
```

```bash
$ ctx sync filesystem
sync filesystem
  fetched: 127 items
  upserted documents: 127
  chunks written: 584
ok
```

### Git Connector

Clones and indexes any Git repository. Extracts per-file commit metadata (author, timestamp, SHA) and auto-generates web URLs for GitHub/GitLab repos.

```toml
[connectors.git]
url = "https://github.com/acme/platform.git"
branch = "main"
root = "docs/"                          # Subdirectory scope
include_globs = ["**/*.md", "**/*.rst"]
exclude_globs = []
shallow = true                           # --depth 1 clone (saves disk)
cache_dir = "./data/.git-cache/platform" # Reuse between syncs
```

```bash
$ ctx sync git
sync git
  cloning https://github.com/acme/platform.git (shallow)...
  fetched: 89 items
  upserted documents: 89
  chunks written: 412
ok

# Subsequent syncs pull incrementally:
$ ctx sync git
sync git
  pulling latest...
  fetched: 3 items (changed)
  upserted documents: 3
ok
```

**What it gives you:**
- Per-file last commit timestamp and author from `git log`
- GitHub/GitLab web URLs auto-generated for each file (clickable in search results)
- Shallow clone support to minimize disk and CI time
- Checkpoint-based incremental sync

### S3 Connector

Indexes documents from Amazon S3 or S3-compatible services (MinIO, LocalStack).

```toml
[connectors.s3]
bucket = "acme-docs"
prefix = "engineering/runbooks/"
region = "us-east-1"
include_globs = ["**/*.md", "**/*.json"]
# endpoint_url = "http://localhost:9000"  # For MinIO/LocalStack
```

Requires `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` environment variables.

```bash
$ ctx sync s3
sync s3
  listing s3://acme-docs/engineering/runbooks/...
  fetched: 34 items
  upserted documents: 34
  chunks written: 156
ok
```

**Features:** pagination for large buckets, `LastModified`/`ETag` tracking, custom endpoints for S3-compatible services.

### Syncing multiple sources

You can configure all connectors in one config and sync them independently:

```bash
$ ctx sync filesystem    # Local docs
$ ctx sync git           # Remote repo
$ ctx sync s3            # S3 bucket

# Or sync a Lua scripted connector:
$ ctx sync script:jira
```

All documents land in the same SQLite database and are searchable together. The `source` field tracks where each document came from.
