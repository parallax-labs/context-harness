FROM rust:1-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y git pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo install --path .

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y ca-certificates curl git && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/ctx /usr/local/bin/ctx
COPY config/ /app/config/
COPY connectors/ /app/connectors/
COPY tools/ /app/tools/
COPY agents/ /app/agents/

WORKDIR /app
RUN mkdir -p /app/data
EXPOSE 7331

HEALTHCHECK --interval=30s --timeout=5s \
  CMD curl -f http://localhost:7331/health || exit 1

ENTRYPOINT ["/bin/bash", "-c"]
CMD ["ctx init --config /app/config/ctx.toml && \
      ctx sync all --full --config /app/config/ctx.toml && \
      ctx embed pending --config /app/config/ctx.toml || true && \
      ctx serve mcp --config /app/config/ctx.toml"]
