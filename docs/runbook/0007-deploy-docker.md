# RUNBOOK-0007: Deploy with Docker

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

This runbook deploys Context Harness as a Docker container. Use it when running the MCP server in a containerized environment, for CI/CD pipelines, or for consistent deployment across hosts.

## Prerequisites

- Docker installed and running
- Git (to clone the repo, or access to a built image)
- (Optional) API keys for embeddings: `OPENAI_API_KEY`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` (for S3 connectors)

## Steps

1. Clone the repository (if building from source).

   ```bash
   git clone https://github.com/parallax-labs/context-harness.git
   cd context-harness
   ```

   Expected: Repository contents in the current directory.

2. Build the Docker image.

   ```bash
   docker build -t context-harness:latest .
   ```

   Expected output (or similar):

   ```
   [internal] load build context
   ...
   => [builder 5/5] RUN cargo install --path .
   ...
   => exporting to image
   => => writing image sha256:...
   => => naming to docker.io/library/context-harness:latest
   ```

3. Run `ctx init` to initialize the database (one-time setup). Use a temporary container or a volume-backed run.

   ```bash
   docker run --rm -v context-harness-data:/app/data context-harness:latest \
     ctx init --config /app/config/ctx.toml
   ```

   Expected output (or similar):

   ```
   Initialized database at /app/data/ctx.sqlite
   ```

4. Run `ctx sync all --full` to perform an initial full sync (optional; the default CMD does this automatically on startup).

   ```bash
   docker run --rm -v context-harness-data:/app/data context-harness:latest \
     ctx sync all --full --config /app/config/ctx.toml
   ```

   Expected output (or similar):

   ```
   Syncing connectors...
   ...
   ```

5. Run the container with the MCP server. The default CMD runs init, sync, embed, and serve.

   ```bash
   docker run -d \
     --name context-harness \
     -p 7331:7331 \
     -v context-harness-data:/app/data \
     -e OPENAI_API_KEY="${OPENAI_API_KEY:-}" \
     context-harness:latest
   ```

   Expected: Container starts; `docker ps` shows `context-harness` running.

6. Perform a health check.

   ```bash
   curl http://localhost:7331/health
   ```

   Expected output (or similar):

   ```
   {"status":"ok"}
   ```

7. (Optional) Use a custom config file. Mount your config and override the default paths.

   ```bash
   docker run -d \
     --name context-harness \
     -p 7331:7331 \
     -v context-harness-data:/app/data \
     -v /path/to/your/ctx.toml:/app/config/ctx.toml:ro \
     -e OPENAI_API_KEY="${OPENAI_API_KEY:-}" \
     context-harness:latest
   ```

   Expected: Container runs with your custom config.

8. (Optional) Pass environment variables for API keys.

   ```bash
   docker run -d \
     --name context-harness \
     -p 7331:7331 \
     -v context-harness-data:/app/data \
     -e OPENAI_API_KEY="sk-..." \
     -e AWS_ACCESS_KEY_ID="..." \
     -e AWS_SECRET_ACCESS_KEY="..." \
     context-harness:latest
   ```

   Expected: Connectors that require these keys (e.g., OpenAI embeddings, S3) can authenticate.

## Volume management

- **Data volume:** The container stores the SQLite database and embeddings in `/app/data`. Use a named volume (`context-harness-data`) or bind mount to persist data.
- **List volumes:** `docker volume ls`
- **Inspect volume:** `docker volume inspect context-harness-data`
- **Remove volume (destructive):** `docker volume rm context-harness-data`

## Verification

- `curl http://localhost:7331/health` returns `{"status":"ok"}`.
- MCP endpoint is reachable: `curl -s http://127.0.0.1:7331/mcp` (may return JSON-RPC or connection info).
- `docker ps` shows the container running; `docker logs context-harness` shows no fatal errors.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `curl: connection refused` on port 7331 | Container not running or port not mapped | Run `docker ps`; ensure `-p 7331:7331` is used; check `docker logs context-harness` |
| Health check fails in Docker | Config binds to wrong address or port | Ensure config `[server] bind` includes `0.0.0.0` or `127.0.0.1` and port 7331 |
| `ctx init` fails: "database exists" | Already initialized | Safe to ignore; proceed to sync and serve |
| Sync fails with auth errors | Missing API keys | Pass `-e OPENAI_API_KEY`, `-e AWS_ACCESS_KEY_ID`, `-e AWS_SECRET_ACCESS_KEY` as needed |
| Container exits immediately | Init/sync/embed error in CMD | Run `docker logs context-harness`; run init/sync manually in a temporary container to isolate the failure |

## Related Runbooks

- [RUNBOOK-0002](0002-build-cli.md) — Build the CLI (for local development)
- [RUNBOOK-0008](0008-deploy-systemd.md) — Deploy with systemd (bare metal)
- [RUNBOOK-0009](0009-deploy-mcp-cursor.md) — Configure Cursor to use the MCP server
