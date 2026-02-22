#!/usr/bin/env bash
# build-docs.sh — Build the Context Harness documentation site with Zola.
#
# 1. Builds the ctx binary
# 2. Generates rustdoc API reference → site/static/api/
# 3. Indexes the repo's docs + source via the Git connector
# 4. Exports the search index as data.json → site/static/docs/data.json
# 5. Builds the Zola site → site/public/
#
# The docs pages use ctx-search.js to provide ⌘K search over the exported index.
#
# Usage:
#   ./scripts/build-docs.sh
#
# Environment:
#   OPENAI_API_KEY  — (optional) enables embedding generation
#   CTX_BINARY      — (optional) path to ctx binary (default: ./target/release/ctx)
#   SKIP_CARGO      — (optional) set to "1" to skip cargo build steps
#   SKIP_ZOLA       — (optional) set to "1" to skip zola build step

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

CTX="${CTX_BINARY:-./target/release/ctx}"
SITE_DIR="./site"
STATIC_DIR="$SITE_DIR/static"
DOCS_DB="$SITE_DIR/docs/ctx-docs.sqlite"
DOCS_DATA="$STATIC_DIR/docs/data.json"
API_DIR="$STATIC_DIR/api"

# ── Step 1: Build ctx binary ──
if [ "${SKIP_CARGO:-}" != "1" ]; then
    echo "==> Building ctx binary..."
    cargo build --release 2>&1

    echo "==> Generating rustdoc..."
    cargo doc --no-deps --document-private-items 2>&1
    mkdir -p "$API_DIR"
    cp -r target/doc/* "$API_DIR/"
else
    echo "==> Skipping cargo build (SKIP_CARGO=1)"
fi

# ── Step 2: Determine repo URL ──
if [ -n "${REPO_URL:-}" ]; then
    GIT_URL="$REPO_URL"
elif git remote get-url origin &>/dev/null; then
    GIT_URL="$(git remote get-url origin)"
else
    GIT_URL="https://github.com/parallax-labs/context-harness.git"
fi
GIT_BRANCH="${GITHUB_REF_NAME:-main}"

echo "==> Indexing docs (repo: $GIT_URL, branch: $GIT_BRANCH)..."

# ── Step 3: Create temporary config & index docs ──
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

[connectors.git.repo]
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
"$CTX" --config "$DOCS_CONFIG" sync git:repo --full

if [ -n "${OPENAI_API_KEY:-}" ]; then
    "$CTX" --config "$DOCS_CONFIG" embed pending || echo "    Warning: embedding failed (non-fatal)"
fi

# ── Step 4: Export data.json ──
echo "==> Exporting data.json..."
mkdir -p "$(dirname "$DOCS_DATA")"

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

# Cleanup temp files
rm -f "$DOCS_CONFIG" "$DOCS_DB" "${DOCS_DB}-wal" "${DOCS_DB}-shm"
rm -rf "$CACHE_DIR"

# ── Step 5: Copy static assets that may not be tracked ──
# Ensure ctx-search.js is in static/
if [ -f "$SITE_DIR/ctx-search.js" ]; then
    cp "$SITE_DIR/ctx-search.js" "$STATIC_DIR/ctx-search.js"
fi

# ── Step 6: Build Zola site ──
if [ "${SKIP_ZOLA:-}" != "1" ]; then
    echo "==> Building Zola site..."
    cd "$SITE_DIR"
    zola build
    cd "$ROOT_DIR"
    echo "    Output: $SITE_DIR/public/"
else
    echo "==> Skipping Zola build (SKIP_ZOLA=1)"
fi

echo "==> Done!"
echo "    site/public/         — complete built site"
echo "    site/public/docs/    — documentation pages"
echo "    site/public/api/     — rustdoc API reference"
echo "    site/public/demo/    — interactive demo"
echo "    site/static/docs/data.json — search index ($DOC_COUNT docs)"
