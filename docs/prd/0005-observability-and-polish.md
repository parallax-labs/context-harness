# PRD-0005: Observability and Polish

**Status:** Delivered
**Date:** 2026-02-24 (implemented) / 2026-02-27 (documented)
**Author:** pjones

## Problem Statement

Context Harness reached feature parity with its marketing site claims but
lacked the operational and diagnostic tools users need to trust and tune
the system:

1. **No visibility into the index.** Users could not answer "how many
   documents are indexed?", "when was the last sync?", or "which sources
   have embeddings?"
2. **Search is a black box.** When a search result feels wrong, users
   have no way to understand why -- is it a keyword problem, a semantic
   problem, or an alpha weighting problem?
3. **No native export.** Exporting the index for the browser search
   widget required a Python one-liner, breaking the single-binary story.
4. **No shell completions.** Tab completion for commands and flags was
   missing.
5. **No container story.** The Dockerfile and docker-compose claims on
   the site were unimplemented.

## Target Users

1. **Operators** deploying Context Harness who need to monitor index
   health and sync status.
2. **Users tuning retrieval** who need to understand scoring and diagnose
   poor results.
3. **CI/CD pipelines** that export the index for static site search.
4. **Docker users** who want a container-based deployment.

## Goals

1. Provide `ctx stats` for index health: document/chunk/embedding counts,
   per-source breakdown, last sync timestamps.
2. Provide `ctx search --explain` for scoring transparency: keyword score,
   semantic score, hybrid score, alpha, and candidate pool sizes per result.
3. Provide `ctx export` as a native replacement for the Python export
   one-liner, producing JSON suitable for the browser search widget.
4. Provide `ctx completions` for generating shell completions (bash, zsh,
   fish) via `clap_complete`.
5. Provide a multi-stage Dockerfile and production-ready docker-compose.yml.
6. Update all documentation to reflect the new commands and container
   deployment.

## Non-Goals

- Prometheus/Grafana metrics endpoint (future work if demand warrants).
- Real-time monitoring dashboard.
- Alerting on sync failures.

## User Stories

**US-1: Check index health.**
An operator runs `ctx stats` and sees: 1,247 documents, 48,392 chunks,
45,100 embeddings, 3 sources, last sync 2 hours ago. They confirm
embeddings are up to date.

**US-2: Diagnose a bad search result.**
A user searches for "auth token validation" and the top result is
irrelevant. They re-run with `--explain` and see the result has a high
keyword score (0.89) but low semantic score (0.12). They realize the
document matches keywords but is about a different kind of token.

**US-3: Export for static site search.**
A CI job runs `ctx export --output data.json` after sync. The JSON file
is deployed alongside the docs site, powering `ctx-search.js`.

**US-4: Docker deployment.**
An operator runs `docker-compose up` with the provided `docker-compose.yml`.
The container persists the SQLite database to a volume, exposes the MCP
server on port 7331, and includes a health check.

**US-5: Shell completions.**
A developer runs `ctx completions zsh > ~/.zfunc/_ctx` and gets tab
completion for all commands, subcommands, and flags.

## Requirements

1. `ctx stats` SHALL display: total documents, total chunks, total
   embeddings, per-source document count, per-source last sync timestamp.
2. `ctx search --explain` SHALL include per-result: keyword_score,
   semantic_score, hybrid_score, hybrid_alpha, keyword_candidates count,
   and vector_candidates count.
3. `ctx export` SHALL produce a JSON file containing document metadata
   and chunk text, suitable for the browser search widget.
4. `ctx completions <shell>` SHALL generate shell completion scripts for
   bash, zsh, and fish.
5. The Dockerfile SHALL use a multi-stage build (builder + runtime) to
   minimize image size.
6. The docker-compose.yml SHALL include persistent volume for the SQLite
   database, port mapping, and a health check.

## Success Criteria

- `ctx stats` completes in <1 second on a 50K-chunk database.
- `ctx search --explain` output includes all scoring components for
  every result.
- `ctx export` produces valid JSON that `ctx-search.js` can consume.
- The Docker image builds successfully and the container runs the MCP
  server with persistent storage.
- Shell completions work for all subcommands and flags.

## Dependencies and Risks

- **Explain output format:** The `--explain` output must be stable enough
  for users to script against. JSON output mode is recommended.
- **Docker base image:** Must not require OpenSSL (using rustls per
  ADR-0017) to avoid runtime dependency issues in minimal containers.

## Related Documents

- **Specs:** [USAGE.md](../USAGE.md) (command documentation),
  [DEPLOYMENT.md](../DEPLOYMENT.md) (Docker and deployment guide),
  [HYBRID_SCORING.md](../HYBRID_SCORING.md) (scoring details
  surfaced by `--explain`)
- **PRDs:** [PRD-0001](0001-core-context-engine.md) (core engine),
  [PRD-0002](0002-browser-search-widget.md) (`ctx export` feeds the
  search widget)
