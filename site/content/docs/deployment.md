+++
title = "Deployment"
description = "Deploy Context Harness in CI/CD, Docker, and production environments."
weight = 10

[extra]
sidebar_label = "Deployment"
sidebar_group = "Reference"
sidebar_order = 10
+++

Context Harness is a single binary that runs anywhere Rust compiles. This guide covers common deployment patterns.

## CI/CD Pipeline

The most common pattern: build the index in CI and deploy a static search index alongside your documentation.

### GitHub Actions Example

```yaml
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

      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/
            target/
          key: cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Build and index docs
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: |
          cargo build --release
          chmod +x scripts/build-docs.sh
          ./scripts/build-docs.sh

      - name: Deploy
        uses: actions/deploy-pages@v4
```

### What `build-docs.sh` Does

1. Builds the `ctx` binary
2. Generates rustdoc API reference → `site/api/`
3. Indexes the repo's docs + source via the Git connector
4. Exports the index as `data.json` → `site/docs/data.json`
5. Cleans up temporary files

## Docker

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

Build and run:

```bash
$ docker build -t context-harness .
$ docker run -p 7331:7331 \
    -e OPENAI_API_KEY=$OPENAI_API_KEY \
    -v ./data:/app/data \
    context-harness
```

## Systemd Service

For long-running MCP server on Linux:

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

## Static Site (Browser-Only)

For documentation sites that need search but no server:

1. Run `scripts/build-docs.sh` in CI to generate `data.json`
2. Include `ctx-search.js` in your HTML
3. The search widget runs entirely in the browser

```html
<script src="/ctx-search.js"></script>
<script>
  window.CTX_SEARCH_DATA = '/data.json';
</script>
```

No server needed at runtime — all search happens client-side over the exported index.

## Production Checklist

- [ ] Set `OPENAI_API_KEY` as a CI secret (not in code)
- [ ] Use `--release` builds for performance
- [ ] Cache cargo artifacts in CI for faster builds
- [ ] Set `shallow = true` on Git connectors in CI
- [ ] Use `[server].bind = "0.0.0.0:7331"` for Docker
- [ ] Monitor `/health` endpoint
- [ ] Rotate API keys periodically

