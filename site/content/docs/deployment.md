+++
title = "Deployment"
description = "Deploy Context Harness in CI/CD, Docker, and production."
weight = 10

[extra]
sidebar_label = "Deployment"
sidebar_group = "Reference"
sidebar_order = 10
+++

### CI/CD — build the index at deploy time

The most common pattern: build the search index in CI and deploy it as a static asset alongside your docs.

```yaml
# .github/workflows/docs.yml
name: Build & Deploy Docs
on:
  push:
    branches: [main]

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

      - name: Build and index
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: |
          cargo build --release
          ./target/release/ctx init
          ./target/release/ctx sync git --full
          ./target/release/ctx embed pending  # Optional

      - name: Deploy
        uses: actions/deploy-pages@v4
```

### Docker

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ctx /usr/local/bin/ctx
COPY config/ /app/config/

WORKDIR /app
ENTRYPOINT ["ctx"]
CMD ["serve", "mcp", "--config", "/app/config/ctx.toml"]
```

```bash
$ docker build -t context-harness .
$ docker run -p 7331:7331 \
    -e OPENAI_API_KEY=$OPENAI_API_KEY \
    -v ./data:/app/data \
    context-harness
```

### Static site search (no server)

For docs sites that need search but no backend:

1. Run `ctx sync` + export `data.json` in CI
2. Include `ctx-search.js` in your HTML
3. Search runs entirely in the browser — no server needed at runtime

```html
<script src="/ctx-search.js"
    data-json="/data.json"
    data-placeholder="Search docs...">
</script>
```

### Systemd service

For a persistent MCP server on Linux:

```ini
[Unit]
Description=Context Harness MCP Server
After=network.target

[Service]
Type=simple
User=ctx
ExecStart=/usr/local/bin/ctx serve mcp --config /etc/ctx/ctx.toml
Restart=on-failure
RestartSec=5
Environment=OPENAI_API_KEY=sk-...

[Install]
WantedBy=multi-user.target
```

### Production checklist

- Set `OPENAI_API_KEY` as a CI secret (never in code)
- Use `--release` builds for performance
- Cache cargo artifacts in CI for faster builds
- Set `shallow = true` on Git connectors in CI
- Use `[server].bind = "0.0.0.0:7331"` for Docker
- Monitor the `/health` endpoint
- Rotate API keys periodically
