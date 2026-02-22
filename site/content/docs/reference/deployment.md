+++
title = "Deployment"
description = "Deploy Context Harness as a persistent MCP server, in Docker, CI/CD, or as a static search index."
weight = 5
+++

### Running the MCP server

The simplest deployment: run `ctx serve mcp` as a persistent process.

```bash
$ ctx serve mcp --config ./config/ctx.toml
Loaded 2 Lua tool(s):
  POST /tools/echo — Echoes back the input message
  POST /tools/create_jira_ticket — Create a Jira ticket enriched with related context
Listening on 127.0.0.1:7331
```

The server binds to the address in `[server].bind`. For Docker or remote access, use `0.0.0.0`:

```toml
[server]
bind = "0.0.0.0:7331"
```

### Docker deployment

Build a minimal Docker image:

```dockerfile
# Dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y git ca-certificates python3 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ctx /usr/local/bin/ctx
COPY config/ /app/config/
COPY connectors/ /app/connectors/
COPY tools/ /app/tools/

WORKDIR /app
EXPOSE 7331

# Initialize, sync, and serve
ENTRYPOINT ["/bin/bash", "-c"]
CMD ["ctx init --config /app/config/ctx.toml && \
      ctx sync git --full --config /app/config/ctx.toml && \
      ctx embed pending --config /app/config/ctx.toml || true && \
      ctx serve mcp --config /app/config/ctx.toml"]
```

```bash
$ docker build -t context-harness .

$ docker run -d \
    --name ctx \
    -p 7331:7331 \
    -e OPENAI_API_KEY=$OPENAI_API_KEY \
    -v ctx-data:/app/data \
    context-harness

$ curl localhost:7331/health
{"status":"ok"}
```

### Docker Compose

For a more complete setup with persistent storage and auto-restart:

```yaml
# docker-compose.yml
version: '3.8'
services:
  context-harness:
    build: .
    ports:
      - "7331:7331"
    volumes:
      - ctx-data:/app/data
      - ./config:/app/config
      - ./connectors:/app/connectors
      - ./tools:/app/tools
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - JIRA_API_TOKEN=${JIRA_API_TOKEN}
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:7331/health"]
      interval: 30s
      timeout: 10s
      retries: 3

volumes:
  ctx-data:
```

```bash
$ docker compose up -d
$ docker compose logs -f context-harness
```

### Systemd service (Linux)

For bare-metal deployments:

```ini
# /etc/systemd/system/context-harness.service
[Unit]
Description=Context Harness MCP Server
After=network.target
Documentation=https://parallax-labs.github.io/context-harness/docs/

[Service]
Type=simple
User=ctx
Group=ctx
WorkingDirectory=/opt/context-harness
ExecStartPre=/usr/local/bin/ctx sync git --full --config /opt/context-harness/config/ctx.toml
ExecStart=/usr/local/bin/ctx serve mcp --config /opt/context-harness/config/ctx.toml
Restart=on-failure
RestartSec=5
Environment=OPENAI_API_KEY=sk-...
Environment=JIRA_API_TOKEN=...

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/opt/context-harness/data
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

```bash
$ sudo systemctl enable context-harness
$ sudo systemctl start context-harness
$ sudo journalctl -u context-harness -f
```

### CI/CD — build the search index

Build the index in CI and deploy it as an artifact or alongside your documentation:

```yaml
# .github/workflows/build-context.yml
name: Build Context Index
on:
  push:
    branches: [main]
    paths: ['docs/**', 'src/**']
  schedule:
    - cron: '0 6 * * *'  # Daily at 6 AM

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/
            target/
          key: cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Build ctx
        run: cargo build --release

      - name: Index documentation
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: |
          ./target/release/ctx init --config ./config/ctx.toml
          ./target/release/ctx sync git --full --config ./config/ctx.toml
          ./target/release/ctx sync filesystem --config ./config/ctx.toml
          ./target/release/ctx embed pending --config ./config/ctx.toml

      - name: Upload database
        uses: actions/upload-artifact@v4
        with:
          name: ctx-database
          path: data/ctx.sqlite
          retention-days: 30
```

### Static site search (no server)

For documentation sites that need search without running a backend:

1. Export the index as `data.json` in CI
2. Include `ctx-search.js` in your HTML
3. All search runs in the browser at zero cost

```bash
# In your build script:
python3 -c "
import sqlite3, json, sys
db = sqlite3.connect('data/ctx.sqlite')
db.row_factory = sqlite3.Row
docs = [dict(r) for r in db.execute('SELECT id, source, source_id, source_url, title, updated_at, body FROM documents')]
chunks = [dict(r) for r in db.execute('SELECT id, document_id, chunk_index, text FROM chunks')]
json.dump({'documents': docs, 'chunks': chunks}, sys.stdout)
" > site/data.json
```

```html
<!-- In your HTML -->
<script src="/ctx-search.js"
    data-json="/data.json"
    data-trigger="#search-btn"
    data-placeholder="Search docs...">
</script>
```

### Cron-based re-sync

Keep the index fresh with periodic re-syncs:

```bash
# /etc/cron.d/context-harness
# Re-sync every 6 hours
0 */6 * * * ctx /usr/local/bin/ctx sync git --config /opt/context-harness/config/ctx.toml && /usr/local/bin/ctx embed pending --config /opt/context-harness/config/ctx.toml
```

Or use a systemd timer:

```ini
# /etc/systemd/system/ctx-sync.timer
[Unit]
Description=Context Harness periodic sync

[Timer]
OnCalendar=*-*-* 0/6:00:00
Persistent=true

[Install]
WantedBy=timers.target
```

### Production checklist

- [ ] **Secrets** — `OPENAI_API_KEY` in CI secrets or env files, never in code
- [ ] **Release builds** — always `cargo build --release` for production
- [ ] **Cargo cache** — cache `~/.cargo` and `target/` in CI for 2-5x faster builds
- [ ] **Shallow clones** — set `shallow = true` on Git connectors to save disk and time
- [ ] **Bind address** — `0.0.0.0:7331` for Docker, `127.0.0.1:7331` for local-only
- [ ] **Health check** — monitor `GET /health` with your uptime tool
- [ ] **Persistent storage** — mount a volume for `data/ctx.sqlite` in Docker
- [ ] **Re-sync schedule** — cron or timer to keep the index fresh
- [ ] **Key rotation** — rotate API keys periodically
- [ ] **Firewall** — restrict access to the MCP server port in production
