#!/usr/bin/env bash
# build-docs.sh — Build documentation assets for the Context Harness site.
#
# 1. Builds the ctx binary
# 2. Generates rustdoc API reference → site/api/
# 3. Indexes the repo's docs + source via the Git connector
# 4. Exports the index as data.json → site/docs/data.json
#
# The docs page uses ctx-search.js to load data.json and provide ⌘K search.
#
# Usage:
#   ./scripts/build-docs.sh
#
# Environment:
#   OPENAI_API_KEY  — (optional) enables embedding generation
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
GIT_BRANCH="${GITHUB_REF_NAME:-main}"

echo "==> Indexing docs (repo: $GIT_URL, branch: $GIT_BRANCH)..."

# Create temporary config
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

[embedding]
provider = "disabled"
EOF

# Enable embeddings if API key available
if [ -n "${OPENAI_API_KEY:-}" ]; then
    sed -i.bak 's/provider = "disabled"/provider = "openai"\nmodel = "text-embedding-3-small"\ndims = 1536\nbatch_size = 64/' "$DOCS_CONFIG"
    rm -f "${DOCS_CONFIG}.bak"
    echo "    (OpenAI embeddings enabled)"
fi

# Build the index
mkdir -p "$(dirname "$DOCS_DB")"
rm -f "$DOCS_DB" "${DOCS_DB}-wal" "${DOCS_DB}-shm"

"$CTX" --config "$DOCS_CONFIG" init
"$CTX" --config "$DOCS_CONFIG" sync git --full

if [ -n "${OPENAI_API_KEY:-}" ]; then
    "$CTX" --config "$DOCS_CONFIG" embed pending || echo "    Warning: embedding failed (non-fatal)"
fi

echo "==> Exporting data.json..."

python3 -c "
import sqlite3, json, sys
db = sqlite3.connect('$DOCS_DB')
db.row_factory = sqlite3.Row
docs = [dict(row) for row in db.execute('SELECT id, source, source_id, source_url, title, updated_at, body FROM documents ORDER BY source_id')]
chunks = [dict(row) for row in db.execute('SELECT id, document_id, chunk_index, text FROM chunks ORDER BY document_id, chunk_index')]
json.dump({'documents': docs, 'chunks': chunks}, sys.stdout, indent=2)
" > "$DOCS_DATA"

DOC_COUNT=$(python3 -c "import json; d=json.load(open('$DOCS_DATA')); print(len(d['documents']))")
CHUNK_COUNT=$(python3 -c "import json; d=json.load(open('$DOCS_DATA')); print(len(d['chunks']))")
echo "    $DOC_COUNT documents, $CHUNK_COUNT chunks"

# Cleanup
rm -f "$DOCS_CONFIG" "$DOCS_DB" "${DOCS_DB}-wal" "${DOCS_DB}-shm"
rm -rf "$CACHE_DIR"

echo "==> Done!"
echo "    site/docs/index.html   — documentation"
echo "    site/docs/data.json    — search index ($DOC_COUNT docs)"
echo "    site/api/              — rustdoc API reference"
echo "    site/ctx-search.js     — search widget"
