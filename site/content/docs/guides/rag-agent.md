+++
title = "Build a RAG Agent"
description = "Build a Python agent that searches your knowledge base, retrieves context, and generates grounded answers."
weight = 3
+++

This guide walks through building a complete **Retrieval-Augmented Generation (RAG)** agent in Python that uses Context Harness as its knowledge backend. The agent searches your docs, retrieves relevant context, and generates grounded answers — no hallucination.

### What you'll build

A command-line agent that:
1. Takes a natural language question
2. Searches your knowledge base via Context Harness
3. Retrieves the most relevant documents
4. Generates an answer grounded in those documents
5. Cites its sources

### Prerequisites

- Context Harness installed and running (`ctx serve mcp`)
- Python 3.10+ with `pip`
- An OpenAI API key (or any OpenAI-compatible provider)

### Step 1: Set up your knowledge base

If you haven't already, create a config and index your docs:

```bash
$ cat > config/ctx.toml << 'EOF'
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536

[retrieval]
final_limit = 10
hybrid_alpha = 0.6

[server]
bind = "127.0.0.1:7331"

[connectors.git.repo]
url = "https://github.com/your-org/your-repo.git"
branch = "main"
include_globs = ["docs/**/*.md", "src/**/*.rs", "README.md"]
shallow = true
EOF

$ ctx init && ctx sync git:repo && ctx embed pending
$ ctx serve mcp &
```

### Step 2: Install Python dependencies

```bash
$ pip install openai requests
```

### Step 3: Build the agent

```python
#!/usr/bin/env python3
"""rag_agent.py — A RAG agent powered by Context Harness."""

import json
import requests
import openai

CTX_URL = "http://localhost:7331"
client = openai.OpenAI()

# ── Tool definitions (fetched from Context Harness) ──────────

def get_tools():
    """Fetch tool schemas from Context Harness and convert to OpenAI format."""
    resp = requests.get(f"{CTX_URL}/tools/list")
    ctx_tools = resp.json()["tools"]
    return [
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

# ── Tool execution ───────────────────────────────────────────

def call_tool(name: str, arguments: dict) -> dict:
    """Execute a tool via Context Harness HTTP API."""
    if name in ("search", "get", "sources"):
        # Built-in tools use their dedicated endpoints
        endpoint = {"search": "search", "get": "get", "sources": "sources"}[name]
        method = "POST" if name != "sources" else "GET"
        if method == "POST":
            resp = requests.post(f"{CTX_URL}/tools/{endpoint}", json=arguments)
        else:
            resp = requests.get(f"{CTX_URL}/tools/{endpoint}")
    else:
        # Lua tools use the dynamic endpoint
        resp = requests.post(f"{CTX_URL}/tools/{name}", json=arguments)

    return resp.json()

# ── Agent loop ───────────────────────────────────────────────

SYSTEM_PROMPT = """You are a helpful technical assistant with access to a knowledge base.

When answering questions:
1. ALWAYS search the knowledge base first using the 'search' tool
2. Use 'hybrid' mode for natural language questions, 'keyword' for specific terms
3. If a search result looks relevant, use 'get' to retrieve the full document
4. Ground your answers in the retrieved documents
5. Cite your sources with titles and URLs when available
6. If you can't find relevant information, say so honestly

Never make up information that isn't in the knowledge base."""

def agent(question: str) -> str:
    """Run the RAG agent on a question."""
    tools = get_tools()
    messages = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user", "content": question},
    ]

    # Agent loop — allow up to 5 tool calls
    for _ in range(5):
        response = client.chat.completions.create(
            model="gpt-4o",
            messages=messages,
            tools=tools,
            tool_choice="auto",
        )

        msg = response.choices[0].message
        messages.append(msg)

        # If the model wants to call tools
        if msg.tool_calls:
            for tool_call in msg.tool_calls:
                name = tool_call.function.name
                args = json.loads(tool_call.function.arguments)

                print(f"  🔧 Calling {name}({json.dumps(args, indent=2)[:100]}...)")
                result = call_tool(name, args)

                messages.append({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": json.dumps(result),
                })
        else:
            # Model is done — return the final answer
            return msg.content

    return msg.content

# ── CLI interface ────────────────────────────────────────────

if __name__ == "__main__":
    import sys

    if len(sys.argv) > 1:
        question = " ".join(sys.argv[1:])
    else:
        question = input("Ask a question: ")

    print(f"\n🤔 Thinking about: {question}\n")
    answer = agent(question)
    print(f"\n📝 Answer:\n{answer}")
```

### Step 4: Run it

```bash
$ python rag_agent.py "How does the hybrid search scoring work?"

🤔 Thinking about: How does the hybrid search scoring work?

  🔧 Calling search({"query": "hybrid search scoring", "mode": "hybrid", "limit": 5}...)
  🔧 Calling get({"id": "a1b2c3d4-..."}...)

📝 Answer:
Context Harness uses a weighted merge of keyword (BM25/FTS5) and semantic
(vector similarity) scores to produce hybrid search results.

The scoring algorithm works as follows:
1. **Candidate retrieval**: The system fetches the top `candidate_k_keyword`
   results from FTS5 and top `candidate_k_vector` from vector search.
2. **Score normalization**: Both scores are normalized to [0, 1] via min-max scaling.
3. **Weighted merge**: `final_score = (1 - alpha) * keyword + alpha * vector`
   where `hybrid_alpha` defaults to 0.6.

Sources:
- [Search & Retrieval](https://github.com/parallax-labs/context-harness/blob/main/docs/HYBRID_SCORING.md)
```

### Step 5: Add conversation memory

Extend the agent to support multi-turn conversations:

```python
def chat():
    """Interactive chat with conversation memory."""
    tools = get_tools()
    messages = [{"role": "system", "content": SYSTEM_PROMPT}]

    print("💬 Chat with your knowledge base (type 'quit' to exit)\n")

    while True:
        question = input("You: ").strip()
        if question.lower() in ("quit", "exit", "q"):
            break

        messages.append({"role": "user", "content": question})

        # Agent loop
        for _ in range(5):
            response = client.chat.completions.create(
                model="gpt-4o",
                messages=messages,
                tools=tools,
                tool_choice="auto",
            )

            msg = response.choices[0].message
            messages.append(msg)

            if msg.tool_calls:
                for tc in msg.tool_calls:
                    result = call_tool(
                        tc.function.name,
                        json.loads(tc.function.arguments),
                    )
                    messages.append({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": json.dumps(result),
                    })
            else:
                print(f"\nAssistant: {msg.content}\n")
                break
```

### Step 6: Use with Anthropic / Claude

The same pattern works with Anthropic's API:

```python
import anthropic

client = anthropic.Anthropic()

# Convert OpenAI tool format to Anthropic format
def openai_to_anthropic_tools(tools):
    return [
        {
            "name": t["function"]["name"],
            "description": t["function"]["description"],
            "input_schema": t["function"]["parameters"],
        }
        for t in tools
    ]

response = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=4096,
    system=SYSTEM_PROMPT,
    tools=openai_to_anthropic_tools(get_tools()),
    messages=[{"role": "user", "content": "How do I deploy this?"}],
)
```

### What's next?

- Add streaming responses for real-time output
- Build a web UI with FastAPI or Flask
- Add authentication for multi-user access
- Deploy with Docker (see [Deployment](/docs/reference/deployment/))
- Add custom Lua tools for write operations (see [Lua Tools](/docs/connectors/lua-tools/))

