+++
title = "Recipe: Chat With Your Blog"
description = "Combine a WordPress connector with a custom agent to create a conversational persona grounded in your own published writing."
date = 2026-02-22

[taxonomies]
tags = ["recipe", "agents"]
+++

Your friend Dave has a decade of blog posts on WordPress. He wants to ask his AI assistant questions like *"What did I write about microservices?"* and get answers sourced from his actual posts — not the internet, not hallucinations, just Dave's own words.

Here's how to set that up in five minutes.

### The idea

1. A Lua connector parses Dave's WordPress XML export into individual posts (see [Recipe: Index Your WordPress Blog](/blog/index-your-wordpress-blog/))
2. An inline agent defines a persona that answers *as Dave* using only his writing
3. Cursor (or Claude Desktop, or any MCP client) connects to the agent and Dave can chat with his own blog

### The config

One file does everything — `config/ctx.toml`:

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[retrieval]
final_limit = 10
hybrid_alpha = 0.6

[server]
bind = "127.0.0.1:7331"

# ── Connector: ingest Dave's blog ────────────────────────────
[connectors.script.wordpress]
path = "connectors/wordpress.lua"
file = "wordpress-export.xml"

# ── Agent: chat with Dave ────────────────────────────────────
[agents.dave]
description = "Chat with Dave — answers based on his published blog posts"
tools = ["search", "get"]
system_prompt = """
You are Dave — a conversational persona grounded entirely in Dave's
published blog posts. When answering questions:

1. Use the `search` tool to find relevant posts
2. Use the `get` tool to read full post content when snippets aren't enough
3. Quote or paraphrase Dave's actual writing — cite post titles
4. If Dave never wrote about a topic, say so honestly
5. Match Dave's writing style — casual, opinionated, practical

Never invent facts. Never attribute ideas to Dave that aren't in his posts.
You ARE Dave, speaking from your own published work.
"""
```

### Set it up

```bash
# Put the WordPress export next to the connector script
cp ~/Downloads/wordpress-export.xml connectors/

# Initialize, sync, and optionally embed
ctx init
ctx sync script:wordpress
ctx embed pending   # optional: enables semantic search

# Start the server
ctx serve mcp
```

### Connect Cursor

`.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "dave": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

Open Cursor and select the *dave* agent. Now you can ask:

- *"What's your take on microservices?"*
- *"Did you ever write about database migrations?"*
- *"Summarize your Kubernetes series"*
- *"What changed in your thinking about testing between 2022 and 2024?"*

Every answer is grounded in Dave's actual posts, with citations.

### Make it smarter with a Lua agent

The inline TOML agent above is static — it always uses the same system prompt. If you want the agent to *pre-fetch* relevant posts before the conversation starts (RAG priming), use a Lua agent instead:

```toml
[agents.script.dave]
path = "agents/dave.lua"
timeout = 30
search_limit = 5
```

```lua
-- agents/dave.lua
agent = {}
agent.name = "dave"
agent.description = "Chat with Dave — grounded in his published blog posts"
agent.tools = { "search", "get" }

agent.arguments = {
    {
        name = "topic",
        description = "Optional topic to pre-load relevant posts",
        required = false,
    },
}

function agent.resolve(args, config, context)
    local topic = args.topic or "recent writing"

    -- Pre-search for relevant posts
    local results = context.search({
        query = topic,
        mode = "hybrid",
        limit = config.search_limit or 5,
    })

    -- Build context from top results
    local context_text = ""
    for _, r in ipairs(results) do
        local doc = context.get(r.id)
        context_text = context_text .. "\n\n---\n## " ..
            (doc.title or "Untitled") .. "\n" .. doc.body
    end

    return {
        system = string.format([[
You are Dave. You answer questions based on your blog posts.

Here are your most relevant posts on "%s":
%s

Use the search and get tools to find more posts if needed.
Quote your own writing. Cite post titles. Be yourself.
        ]], topic, context_text),

        messages = {
            {
                role = "assistant",
                content = string.format(
                    "I've loaded %d of my posts about %s. What do you want to know?",
                    #results, topic
                ),
            },
        },
    }
end

return agent
```

Now when someone selects the *dave* agent with `topic=kubernetes`, the agent pre-searches for Kubernetes posts and injects them into the system prompt before the conversation even starts.

### Beyond WordPress

This pattern works for any personal writing:

| Source | Connector | Agent persona |
|--------|-----------|---------------|
| WordPress XML export | `wordpress.lua` | "Chat with Dave" |
| Ghost blog JSON export | `ghost.lua` | "Chat with the author" |
| Markdown files in a repo | Built-in `filesystem` | "Chat with the docs" |
| Substack email archive | `substack.lua` | "Chat with the newsletter" |
| Notion export | `notion.lua` | "Chat with my notes" |

The connector parses the source format. The agent defines the persona. Context Harness handles everything in between.
