#!/usr/bin/env bash
# build-docs.sh — Build searchable documentation for the Context Harness site.
#
# This script dogfoods the Git connector to ingest the project's own docs
# and Rust source files, then exports the indexed data as JSON for the
# browser-based search page. It also generates rustdoc.
#
# Usage:
#   ./scripts/build-docs.sh
#
# Environment:
#   OPENAI_API_KEY  — (optional) enables semantic search embeddings
#   REPO_URL        — (optional) override repo URL (default: current origin)
#   CTX_BINARY      — (optional) path to ctx binary (default: ./target/release/ctx)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

CTX="${CTX_BINARY:-./target/release/ctx}"
DOCS_DB="./site/docs/ctx-docs.sqlite"
DOCS_DATA="./site/docs/data.json"
API_DIR="./site/api"

echo "==> Building ctx binary..."
cargo build --release 2>&1

echo "==> Generating rustdoc..."
cargo doc --no-deps --document-private-items 2>&1
mkdir -p "$API_DIR"
cp -r target/doc/* "$API_DIR/"

# Determine the repo URL
if [ -n "${REPO_URL:-}" ]; then
    GIT_URL="$REPO_URL"
elif git remote get-url origin &>/dev/null; then
    GIT_URL="$(git remote get-url origin)"
else
    GIT_URL="https://github.com/parallax-labs/context-harness.git"
fi

echo "==> Using repo URL: $GIT_URL"

# Determine branch
GIT_BRANCH="${GITHUB_REF_NAME:-main}"
echo "==> Using branch: $GIT_BRANCH"

# Create docs config
DOCS_CONFIG="$(mktemp /tmp/ctx-docs-XXXXXX.toml)"
CACHE_DIR="$(mktemp -d /tmp/ctx-docs-cache-XXXXXX)"

cat > "$DOCS_CONFIG" <<EOF
[db]
path = "$ROOT_DIR/site/docs/ctx-docs.sqlite"

[chunking]
max_tokens = 500
overlap_tokens = 0

[retrieval]
final_limit = 20
hybrid_alpha = 0.6
candidate_k_keyword = 80
candidate_k_vector = 80

[server]
bind = "127.0.0.1:7331"

[connectors.git]
url = "$GIT_URL"
branch = "$GIT_BRANCH"
root = "."
include_globs = ["docs/**/*.md", "src/**/*.rs", "README.md", "CHANGELOG.md", "CONTRIBUTING.md"]
exclude_globs = ["**/target/**"]
shallow = true
cache_dir = "$CACHE_DIR"
EOF

# Embedding config — only if OPENAI_API_KEY is available
if [ -n "${OPENAI_API_KEY:-}" ]; then
    cat >> "$DOCS_CONFIG" <<EOF

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64
EOF
    echo "==> OpenAI embeddings enabled"
else
    cat >> "$DOCS_CONFIG" <<EOF

[embedding]
provider = "disabled"
EOF
    echo "==> Embeddings disabled (no OPENAI_API_KEY)"
fi

# Ensure output directory exists
mkdir -p "$(dirname "$DOCS_DB")"

# Remove old database if it exists
rm -f "$DOCS_DB" "${DOCS_DB}-wal" "${DOCS_DB}-shm"

echo "==> Initializing docs database..."
"$CTX" --config "$DOCS_CONFIG" init

echo "==> Syncing docs via Git connector..."
"$CTX" --config "$DOCS_CONFIG" sync git --full

# Embed if enabled
if [ -n "${OPENAI_API_KEY:-}" ]; then
    echo "==> Generating embeddings..."
    "$CTX" --config "$DOCS_CONFIG" embed pending || echo "Warning: embedding failed (non-fatal)"
fi

echo "==> Exporting data to JSON..."

# Export documents and chunks from SQLite to JSON using sqlite3
# Use a Python one-liner for JSON export since sqlite3 JSON support varies
python3 -c "
import sqlite3, json, sys

db = sqlite3.connect('$DOCS_DB')
db.row_factory = sqlite3.Row

docs = []
for row in db.execute('SELECT id, source, source_id, source_url, title, updated_at, body FROM documents ORDER BY source_id'):
    docs.append({
        'id': row['id'],
        'source': row['source'],
        'source_id': row['source_id'],
        'source_url': row['source_url'],
        'title': row['title'],
        'updated_at': row['updated_at'],
        'body': row['body'],
    })

chunks = []
for row in db.execute('SELECT id, document_id, chunk_index, text FROM chunks ORDER BY document_id, chunk_index'):
    chunks.append({
        'id': row['id'],
        'document_id': row['document_id'],
        'chunk_index': row['chunk_index'],
        'text': row['text'],
    })

json.dump({'documents': docs, 'chunks': chunks}, sys.stdout, indent=2)
" > "$DOCS_DATA"

DOC_COUNT=$(python3 -c "import json; d=json.load(open('$DOCS_DATA')); print(len(d['documents']))")
CHUNK_COUNT=$(python3 -c "import json; d=json.load(open('$DOCS_DATA')); print(len(d['chunks']))")

echo "==> Exported $DOC_COUNT documents, $CHUNK_COUNT chunks"

# Cleanup
rm -f "$DOCS_CONFIG"
rm -rf "$CACHE_DIR"
rm -f "$DOCS_DB" "${DOCS_DB}-wal" "${DOCS_DB}-shm"

echo "==> Done! Files:"
echo "    $DOCS_DATA"
echo "    $API_DIR/"
echo ""
echo "    Docs search page:  site/docs/index.html"
echo "    Rustdoc API:        site/api/context_harness/index.html"

