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

Scans a local directory tree. Each file becomes a Document with its content as the body, path as the source ID, and modification time as the timestamp.

```toml
[connectors.filesystem]
root = "./docs"                        # Directory to scan (required)
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

**Tips:**
- Paths are relative to where you run `ctx`
- Use `exclude_globs` for `target/`, `node_modules/`, `.git/`, build artifacts
- Good for: Obsidian vaults, local project docs, meeting notes, ADRs

**Example — Index an Obsidian vault:**

```toml
[connectors.filesystem]
root = "~/Documents/notes"
include_globs = ["**/*.md"]
exclude_globs = ["**/.obsidian/**", "**/templates/**"]
```

### Git Connector

Clones and indexes any Git repository — local or remote. Extracts per-file commit metadata (author, timestamp, SHA) and auto-generates web URLs for GitHub/GitLab repos.

```toml
[connectors.git]
url = "https://github.com/acme/platform.git"  # Repo URL (required)
branch = "main"                               # Branch to track
root = "docs/"                                 # Subdirectory scope
include_globs = [                              # File patterns to include
    "docs/**/*.md",
    "src/**/*.rs",
    "README.md",
    "CHANGELOG.md",
]
exclude_globs = ["**/target/**"]               # File patterns to exclude
shallow = true                                  # --depth 1 clone (saves disk/time)
cache_dir = "./data/.git-cache/platform"        # Reuse clone between syncs
```

```bash
# First sync — clones the repo
$ ctx sync git
sync git
  cloning https://github.com/acme/platform.git (shallow)...
  fetched: 89 items
  upserted documents: 89
  chunks written: 412
ok

# Second sync — pulls incrementally
$ ctx sync git
sync git
  pulling latest...
  fetched: 3 items (changed)
  upserted documents: 3
ok

# Force full re-sync
$ ctx sync git --full
```

**What it gives you:**
- Per-file last commit timestamp and author from `git log`
- GitHub/GitLab web URLs auto-generated for each file (clickable in search results)
- Shallow clone support for fast CI builds
- Checkpoint-based incremental sync — only re-processes changed files

**Private repos:** Use an SSH URL or a `GITHUB_TOKEN`:

```toml
[connectors.git]
url = "git@github.com:acme/private-repo.git"
# or
url = "https://x-access-token:${GITHUB_TOKEN}@github.com/acme/private-repo.git"
```

**Local repos:** Point at a local path instead of a URL:

```toml
[connectors.git]
url = "/home/user/projects/my-repo"
branch = "main"
```

### S3 Connector

Indexes documents from Amazon S3 or S3-compatible services (MinIO, LocalStack, Backblaze B2, Cloudflare R2).

```toml
[connectors.s3]
bucket = "acme-docs"                       # Bucket name (required)
prefix = "engineering/runbooks/"            # Key prefix filter
region = "us-east-1"                       # AWS region
include_globs = ["**/*.md", "**/*.json"]   # Key patterns to include
# endpoint_url = "http://localhost:9000"   # For MinIO/LocalStack
```

Requires `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` environment variables.

```bash
$ export AWS_ACCESS_KEY_ID="AKIA..."
$ export AWS_SECRET_ACCESS_KEY="..."

$ ctx sync s3
sync s3
  listing s3://acme-docs/engineering/runbooks/...
  fetched: 34 items
  upserted documents: 34
  chunks written: 156
ok
```

**S3-compatible services:**

```toml
# MinIO (local)
[connectors.s3]
bucket = "docs"
region = "us-east-1"
endpoint_url = "http://localhost:9000"

# Cloudflare R2
[connectors.s3]
bucket = "my-docs"
region = "auto"
endpoint_url = "https://ACCOUNT_ID.r2.cloudflarestorage.com"
```

**Features:**
- Automatic pagination for large buckets (1000+ objects)
- `LastModified`/`ETag` tracking for incremental sync
- Custom endpoint URL for any S3-compatible service
- Glob-based include/exclude filtering on object keys

### Combining multiple sources

All connectors feed into the same SQLite database. Documents from different sources are tracked separately and searchable together:

```bash
$ ctx sync filesystem    # Local docs
$ ctx sync git           # Remote repo
$ ctx sync s3            # S3 bucket
$ ctx sync script:jira   # Lua connector

# Search across everything
$ ctx search "deployment" --mode hybrid

# Filter to one source
$ ctx search "deployment" --source git
```

The `source` field in search results tells you where each result came from.

### Embedding configuration

Embeddings enable semantic and hybrid search. Without embeddings, only keyword search (FTS5/BM25) is available — which is still fast and useful.

```toml
[embedding]
provider = "openai"                  # "disabled" or "openai"
model = "text-embedding-3-small"      # OpenAI model name
dims = 1536                           # Vector dimensions
batch_size = 64                       # Texts per API call
max_retries = 5                       # Retry on transient failures
timeout_secs = 30                     # Per-request timeout
```

```bash
$ export OPENAI_API_KEY="sk-..."
$ ctx embed pending
Embedding 584 chunks... done (12.3s)
```

Embedding is non-fatal — documents are always ingested even if embedding fails. You can embed later with `ctx embed pending`.
