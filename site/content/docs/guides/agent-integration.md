+++
title = "Agent Integration"
description = "Step-by-step setup for Cursor, Claude Desktop, Continue.dev, OpenClaw, and custom agents."
weight = 1
+++

Context Harness exposes an MCP-compatible HTTP server that any AI agent can consume. This guide walks through connecting it to the most popular tools.

### Prerequisites

Before connecting any agent, start the MCP server:

```bash
$ ctx serve mcp --config ./config/ctx.toml
Listening on 127.0.0.1:7331
```

Verify it's running:

```bash
$ curl -s localhost:7331/health
{"status":"ok"}

$ curl -s localhost:7331/tools/list | jq '.tools | length'
6
```

---

### Cursor

Cursor supports MCP servers natively. Add Context Harness to your workspace or global settings:

**Option 1: Workspace-level** (recommended for team projects)

Create `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://localhost:7331"
    }
  }
}
```

**Option 2: Global** (available in all projects)

Open Cursor Settings → MCP Servers → Add Server:

```
Name: context-harness
URL:  http://localhost:7331
```

**Using it in Cursor:**

Once connected, Cursor's agent can use your knowledge base naturally:

- *"Search our docs for the deployment procedure"* → calls `POST /tools/search`
- *"Get the full content of the auth module"* → calls `POST /tools/get`
- *"What data sources are configured?"* → calls `GET /tools/sources`
- *"Create a Jira ticket for this bug"* → calls `POST /tools/create_jira_ticket`

The agent discovers all available tools (built-in + Lua extensions) automatically via `GET /tools/list`.

**Pro tip:** For multi-repo workspaces in Cursor, run one Context Harness instance that indexes all repos. See the [Multi-Repo Context](@/docs/guides/multi-repo.md) guide.

---

### Claude Desktop

Claude Desktop supports MCP servers through its configuration file.

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`

**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "context-harness": {
      "command": "ctx",
      "args": ["serve", "mcp", "--config", "/path/to/config/ctx.toml"]
    }
  }
}
```

This launches the MCP server automatically when Claude Desktop starts. Alternatively, if you prefer to manage the server yourself:

```json
{
  "mcpServers": {
    "context-harness": {
      "url": "http://localhost:7331"
    }
  }
}
```

**Using it in Claude Desktop:**

Ask Claude anything about your codebase or docs:

- *"How does authentication work in our platform?"*
- *"Find all references to the payment processing flow"*
- *"What changed in the last deployment?"*

Claude will search your indexed knowledge base and include relevant context in its responses.

---

### Continue.dev

[Continue.dev](https://continue.dev) is an open-source AI code assistant for VS Code and JetBrains.

Add Context Harness as a context provider in `~/.continue/config.json`:

```json
{
  "contextProviders": [
    {
      "name": "http",
      "params": {
        "url": "http://localhost:7331/tools/search",
        "title": "Context Harness",
        "description": "Search project knowledge base",
        "queryParams": {
          "mode": "hybrid"
        }
      }
    }
  ]
}
```

Or use Continue's MCP support directly:

```json
{
  "experimental": {
    "mcpServers": [
      {
        "name": "context-harness",
        "url": "http://localhost:7331"
      }
    ]
  }
}
```

---

### OpenClaw / Open Interpreter

Any tool that supports HTTP tool calling or MCP can connect to Context Harness. The pattern is the same:

1. Start `ctx serve mcp` on a known port
2. Point the tool at `http://localhost:7331`
3. The tool discovers available tools via `GET /tools/list`

For Open Interpreter, you can use the tools directly via its custom tool support:

```python
import requests

def search_context(query: str, mode: str = "hybrid") -> dict:
    """Search the project knowledge base."""
    resp = requests.post("http://localhost:7331/tools/search", json={
        "query": query,
        "mode": mode,
        "limit": 10,
    })
    return resp.json()
```

---

### Custom agents (any language)

Context Harness is just an HTTP API. Any agent in any language can use it:

**Python:**

```python
import requests

BASE = "http://localhost:7331"

# Discover tools
tools = requests.get(f"{BASE}/tools/list").json()["tools"]

# Search
results = requests.post(f"{BASE}/tools/search", json={
    "query": "error handling patterns",
    "mode": "hybrid",
    "limit": 5,
}).json()

# Get full document
doc = requests.post(f"{BASE}/tools/get", json={
    "id": results["results"][0]["id"],
}).json()

# Call a custom Lua tool
ticket = requests.post(f"{BASE}/tools/create_jira_ticket", json={
    "title": "Implement retry logic",
    "priority": "Medium",
}).json()
```

**TypeScript/Node:**

```typescript
const BASE = "http://localhost:7331";

// Search with fetch
const { results } = await fetch(`${BASE}/tools/search`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ query: "auth", mode: "hybrid" }),
}).then(r => r.json());

// Use with OpenAI function calling
const tools = await fetch(`${BASE}/tools/list`).then(r => r.json());
// tools.tools already has OpenAI-compatible JSON Schema
```

**Shell/curl:**

```bash
# Search
curl -s localhost:7331/tools/search \
  -H "Content-Type: application/json" \
  -d '{"query": "deploy", "mode": "keyword"}' | jq '.results[] | {title, score}'

# Get all tools
curl -s localhost:7331/tools/list | jq '.tools[] | {name, description}'

# Call a tool
curl -s -X POST localhost:7331/tools/echo \
  -H "Content-Type: application/json" \
  -d '{"message": "hello"}' | jq .result
```

---

### OpenAI function calling format

The `GET /tools/list` endpoint returns tool schemas in OpenAI-compatible JSON Schema format. This means you can pass them directly to OpenAI's, Anthropic's, or any provider's function calling API:

```python
import openai

# Fetch tool definitions from Context Harness
ctx_tools = requests.get("http://localhost:7331/tools/list").json()["tools"]

# Convert to OpenAI format
openai_tools = [
    {
        "type": "function",
        "function": {
            "name": t["name"],
            "description": t["description"],
            "parameters": t["parameters"],
        },
    }
    for t in ctx_tools
]

# Use in a chat completion
response = openai.chat.completions.create(
    model="gpt-4",
    messages=[{"role": "user", "content": "Find docs about authentication"}],
    tools=openai_tools,
)

# Execute the tool call
if response.choices[0].message.tool_calls:
    call = response.choices[0].message.tool_calls[0]
    result = requests.post(
        f"http://localhost:7331/tools/{call.function.name}",
        json=json.loads(call.function.arguments),
    ).json()
```

