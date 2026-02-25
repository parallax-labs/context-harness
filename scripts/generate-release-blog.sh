#!/usr/bin/env bash
# generate-release-blog.sh — Create a blog post stub for a new release.
#
# Extracts the version from the git tag (or Cargo.toml), generates
# a skeleton blog post in site/content/blog/, and pre-fills it with
# the git log since the last tag.
#
# Usage:
#   ./scripts/generate-release-blog.sh           # uses current HEAD tag
#   ./scripts/generate-release-blog.sh v0.4.0    # explicit version

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

# Determine version
if [ -n "${1:-}" ]; then
    VERSION="$1"
else
    VERSION="$(git describe --tags --abbrev=0 2>/dev/null || grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')"
fi

VERSION_CLEAN="${VERSION#v}"
SLUG="v${VERSION_CLEAN//./-}"
DATE="$(date +%Y-%m-%d)"
BLOG_FILE="site/content/blog/${SLUG}-release.md"

if [ -f "$BLOG_FILE" ]; then
    echo "Blog post already exists: $BLOG_FILE"
    exit 0
fi

# Find the previous tag for changelog
PREV_TAG="$(git tag --sort=-creatordate | grep -v "^${VERSION}$" | head -1 2>/dev/null || echo "")"

# Build commit list
if [ -n "$PREV_TAG" ]; then
    COMMITS="$(git log --oneline "${PREV_TAG}..HEAD" 2>/dev/null || echo "(no commits found)")"
    RANGE="${PREV_TAG}..${VERSION}"
else
    COMMITS="$(git log --oneline -20 2>/dev/null || echo "(no commits found)")"
    RANGE="(initial release)"
fi

# Count stats
FILE_COUNT="$(git diff --stat "${PREV_TAG:-$(git rev-list --max-parents=0 HEAD)}"..HEAD 2>/dev/null | tail -1 || echo "unknown")"

cat > "$BLOG_FILE" <<EOF
+++
title = "v${VERSION_CLEAN}: TODO_TITLE"
description = "TODO_DESCRIPTION"
date = ${DATE}

[taxonomies]
tags = ["release"]
+++

<!-- Auto-generated release blog post for ${VERSION} -->
<!-- Range: ${RANGE} -->
<!-- Edit this file, then commit and push to deploy -->

Context Harness v${VERSION_CLEAN} is out.

### What changed

<!-- Commits since last release:

${COMMITS}

Files changed: ${FILE_COUNT}

-->

TODO: Write the release notes here. Group changes into sections:
- New features
- Improvements
- Bug fixes
- Breaking changes (if any)

### Upgrading

\`\`\`bash
# From source
cargo install --path . --force

# Pre-built binary
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
\`\`\`

### What's next

TODO: What's coming in the next release.
EOF

echo "Created: $BLOG_FILE"
echo "Edit the file, then commit and push to deploy."
