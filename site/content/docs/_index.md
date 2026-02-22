+++
title = "Documentation"
description = "Everything you need to install, configure, and deploy Context Harness — a local-first context engine for AI tools."
sort_by = "weight"
template = "docs/section.html"
page_template = "docs/page.html"
+++

Context Harness is a single Rust binary that ingests documentation, code, and knowledge from any source, stores it in a local SQLite database, and makes it searchable by AI agents through a CLI and MCP-compatible HTTP server. No cloud. No vendor lock-in. One binary.

## What can you do with it?

- **Search your docs** — full-text, semantic, and hybrid search across all your knowledge
- **Connect AI agents** — Cursor, Claude Desktop, Continue.dev, any MCP-compatible tool
- **Index everything** — Git repos, S3 buckets, Jira, Slack, Confluence, Notion (via Lua connectors)
- **Multi-repo context** — one instance indexes multiple repos for unified search
- **Custom tools** — let agents create tickets, post to Slack, trigger deploys (via Lua tools)
- **Static site search** — zero-dependency ⌘K search widget for documentation sites
- **Deploy anywhere** — Docker, systemd, CI/CD, or just run the binary
