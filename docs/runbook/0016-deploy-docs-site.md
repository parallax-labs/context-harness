# RUNBOOK-0016: Deploy Documentation Site

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook covers building and deploying the Context Harness documentation site. The site uses Zola (static site generator) v0.19.2 and is hosted on GitHub Pages. Use this runbook when building the docs locally for preview, deploying to production, or troubleshooting build failures.

## Prerequisites

- Local development environment set up (see [RUNBOOK-0001](0001-local-dev-setup.md))
- Rust toolchain (stable)
- Python 3 (for data.json export and optional site-index generation)
- Git (for the Git connector self-index step)
- (Optional) `OPENAI_API_KEY` — enables embedding generation for semantic search; build works without it

## Steps

### 1. Install Zola

The build script requires Zola v0.19.2. Install via your package manager or the [official install instructions](https://www.getzola.org/documentation/getting-started/installation/).

```bash
# macOS (Homebrew)
brew install zola

# Or download binary from https://github.com/getzola/zola/releases
```

Verify installation:

```bash
zola --version
```

Expected output (or similar):

```
zola 0.19.2
```

### 2. Run the build script

From the repository root:

```bash
./scripts/build-docs.sh
```

Expected output (abbreviated):

```
==> Building ctx binary...
==> Generating rustdoc...
==> Indexing docs (repo: ..., branch: main)...
==> Exporting data.json...
    N documents, M chunks
==> Generating site-index.json...
==> Building Zola site (with site-index for search)...
==> Done!
    site/public/         — complete built site
    site/public/docs/    — documentation pages
    site/public/api/     — rustdoc API reference
    site/public/demo/    — interactive demo
    site/static/docs/data.json — search index (N docs)
```

The build process:

1. Builds the `ctx` binary
2. Generates rustdoc API reference → `site/static/api/`
3. Uses the Git connector to self-index docs and source
4. Exports the search index as `site/static/docs/data.json`
5. Optionally generates embeddings if `OPENAI_API_KEY` is set
6. Builds the Zola site → `site/public/`

### 3. Preview locally

Serve the built site with a local HTTP server:

```bash
cd site && python3 -m http.server 8080
```

Expected: Server starts; visit http://localhost:8080 in a browser.

### 4. Deploy to GitHub Pages

Deployment is automatic via the `.github/workflows/pages.yml` workflow.

**Trigger on push to main:** The workflow runs when changes are pushed to `main` that affect:

- `site/**`
- `crates/**`
- `docs/**`
- `scripts/**`
- `Cargo.toml`
- `Cargo.lock`
- `.github/workflows/pages.yml`

**Manual trigger:** Go to Actions → "Deploy Site & Docs" → "Run workflow".

The workflow:

1. Installs Rust and Zola v0.19.2
2. Runs `./scripts/build-docs.sh`
3. Uploads `site/public` as the Pages artifact
4. Deploys to GitHub Pages

### 5. Verify the live site

After deployment completes (typically 1–3 minutes):

1. Open the Pages URL (e.g. `https://parallax-labs.github.io/context-harness`)
2. Confirm the homepage loads
3. Navigate to `/docs/` and verify documentation pages render
4. Test ⌘K search (or Ctrl+K) — it uses `ctx-search.js` with the exported `data.json`

## Verification

- `./scripts/build-docs.sh` completes without errors
- `site/public/` contains the built site (HTML, CSS, JS, `docs/`, `api/`, `demo/`)
- Local preview at http://localhost:8080 shows the site correctly
- GitHub Actions "Deploy Site & Docs" workflow succeeds
- Live site loads and search works

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `zola: command not found` | Zola not installed | Install Zola v0.19.2; see step 1 |
| `cargo doc` fails | Missing dependencies or workspace issues | Run `cargo build --release -p context-harness` first; ensure workspace resolves |
| Git connector fails during index | No `origin` remote or auth issues | Set `REPO_URL` env var if needed; for private repos, ensure SSH key or credential helper is configured |
| `ctx init` or `ctx sync` fails | See common errors | See [RUNBOOK-0017](0017-common-errors.md) |
| `python3` not found | Python 3 not installed | Install Python 3; the script uses it for data.json export |
| Zola build fails | Invalid templates or config | Check `site/config.toml` and templates; run `zola build` in `site/` for detailed errors |
| Pages workflow fails | CI environment differs from local | Check Actions logs; common causes: cache corruption, missing `OPENAI_API_KEY` (optional), path changes |
| Search returns no results | `data.json` empty or missing | Ensure build-docs.sh completed; check `site/static/docs/data.json` exists and has `documents` and `chunks` arrays |

## Related Runbooks

- [RUNBOOK-0001](0001-local-dev-setup.md) — Local Development Setup
- [RUNBOOK-0017](0017-common-errors.md) — Common Errors Reference (ctx sync, embeddings, etc.)
