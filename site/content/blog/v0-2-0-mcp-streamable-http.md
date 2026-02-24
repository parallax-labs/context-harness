+++
title = "v0.2.0: Native MCP Streamable HTTP"
description = "Context Harness now speaks the MCP protocol natively — connect Cursor, Claude, and other MCP clients directly via URL."
date = 2026-02-24

[taxonomies]
tags = ["release"]
+++

Context Harness v0.2.0 ships with a native MCP Streamable HTTP endpoint, pre-built binaries for all major platforms, and updated docs across the board.

### What changed

**Native MCP protocol support.** The server now exposes a `/mcp` endpoint that speaks the [MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports#streamable-http) transport — the same JSON-RPC-over-HTTP protocol that Cursor, Claude Desktop, and other MCP clients use natively.

Previously, connecting Cursor required either pointing it at the base URL (which only exposed REST endpoints) or using the `command`/`args` subprocess approach. Now it's just a URL:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

**What's exposed via MCP:**

| MCP Method | Maps to |
|-----------|---------|
| `tools/list` | All registered tools (built-in + Lua + Rust) |
| `tools/call` | Execute any tool by name |
| `prompts/list` | All registered agents as MCP prompts |
| `prompts/get` | Resolve an agent's system prompt |

The REST endpoints (`/tools/list`, `/tools/{name}`, `/agents/*`, `/health`) are still available for custom integrations.

### Pre-built binaries

You no longer need Rust installed to use Context Harness. Every release now ships pre-compiled binaries for:

- **Linux** x86_64 (glibc and musl)
- **Linux** aarch64
- **macOS** Intel and Apple Silicon
- **Windows** x86_64

Download from [GitHub Releases](https://github.com/parallax-labs/context-harness/releases/latest):

```bash
curl -L https://github.com/parallax-labs/context-harness/releases/latest/download/ctx-macos-aarch64.tar.gz | tar xz
sudo mv ctx /usr/local/bin/
```

SHA256 checksums are included for every archive.

### Upgrading

If you have existing MCP client configs that use the old `command`/`args` approach or a bare `http://localhost:7331` URL, update them to point to `http://127.0.0.1:7331/mcp`.

Start the server separately:

```bash
ctx serve mcp --config ./config/ctx.toml
```

Then update your `.cursor/mcp.json`, Claude Desktop config, or any other MCP client to use the new URL.

### What's next

- Blog with recipes and real-world use cases (you're reading it)
- More Lua connector examples
- Performance improvements to the vector search path
