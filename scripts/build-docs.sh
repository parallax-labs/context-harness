#!/usr/bin/env bash
# build-docs.sh — Build documentation assets for the Context Harness site.
#
# Generates rustdoc API reference and copies it into site/api/.
# The docs page itself is static HTML committed to site/docs/index.html.
#
# Usage:
#   ./scripts/build-docs.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

API_DIR="./site/api"

echo "==> Building ctx binary..."
cargo build --release 2>&1

echo "==> Generating rustdoc..."
cargo doc --no-deps --document-private-items 2>&1
mkdir -p "$API_DIR"
cp -r target/doc/* "$API_DIR/"

echo "==> Done!"
echo "    Docs page:    site/docs/index.html"
echo "    Rustdoc API:  site/api/context_harness/index.html"
