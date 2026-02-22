+++
title = "Build a Knowledge-Base Chatbot"
description = "Build a TypeScript chatbot with streaming, conversation history, and a web UI."
weight = 4
+++

This guide builds a full-stack chatbot with a web UI that chats with your knowledge base. It uses TypeScript, the OpenAI SDK, and Context Harness for retrieval.

### What you'll build

A browser-based chatbot that:
- Streams responses in real-time
- Automatically searches your knowledge base for relevant context
- Maintains conversation history
- Shows source citations inline
- Works with OpenAI, Anthropic, or any OpenAI-compatible API

### Prerequisites

- Context Harness running (`ctx serve mcp` on port 7331)
- Node.js 18+ and npm
- An OpenAI API key

### Project setup

```bash
$ mkdir ctx-chatbot && cd ctx-chatbot
$ npm init -y
$ npm install openai express
$ mkdir public
```

### Step 1: Backend — Express + OpenAI with tool calling

```typescript
// server.ts
import express from "express";
import OpenAI from "openai";
import { Readable } from "stream";

const app = express();
app.use(express.json());
app.use(express.static("public"));

const openai = new OpenAI();
const CTX_URL = process.env.CTX_URL || "http://localhost:7331";

// ── Fetch tools from Context Harness ────────────────────────

async function getTools(): Promise<OpenAI.ChatCompletionTool[]> {
  const resp = await fetch(`${CTX_URL}/tools/list`);
  const { tools } = (await resp.json()) as { tools: any[] };
  return tools.map((t) => ({
    type: "function" as const,
    function: {
      name: t.name,
      description: t.description,
      parameters: t.parameters,
    },
  }));
}

// ── Execute a tool call ─────────────────────────────────────

async function callTool(name: string, args: Record<string, any>) {
  const isGet = name === "sources";
  const url = `${CTX_URL}/tools/${name}`;

  const resp = isGet
    ? await fetch(url)
    : await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(args),
      });

  return resp.json();
}

// ── System prompt ───────────────────────────────────────────

const SYSTEM = `You are a helpful assistant with access to a knowledge base.

Rules:
1. ALWAYS search the knowledge base before answering technical questions.
2. Use 'hybrid' mode for conceptual questions, 'keyword' for specific terms.
3. Ground your answers in retrieved documents — never make things up.
4. Cite sources inline: [Source Title](url).
5. If you can't find relevant info, say so.
6. Be concise but thorough.`;

// ── Chat endpoint with streaming ────────────────────────────

type Message = OpenAI.ChatCompletionMessageParam;

app.post("/api/chat", async (req, res) => {
  const { messages } = req.body as { messages: Message[] };
  const tools = await getTools();

  const allMessages: Message[] = [
    { role: "system", content: SYSTEM },
    ...messages,
  ];

  res.setHeader("Content-Type", "text/event-stream");
  res.setHeader("Cache-Control", "no-cache");
  res.setHeader("Connection", "keep-alive");

  // Agent loop — allow tool calls
  for (let i = 0; i < 5; i++) {
    const response = await openai.chat.completions.create({
      model: "gpt-4o",
      messages: allMessages,
      tools,
      tool_choice: "auto",
      stream: true,
    });

    let toolCalls: any[] = [];
    let content = "";
    let finishReason = "";

    for await (const chunk of response) {
      const delta = chunk.choices[0]?.delta;
      finishReason = chunk.choices[0]?.finish_reason || finishReason;

      // Accumulate tool calls
      if (delta?.tool_calls) {
        for (const tc of delta.tool_calls) {
          if (!toolCalls[tc.index]) {
            toolCalls[tc.index] = {
              id: tc.id || "",
              function: { name: "", arguments: "" },
            };
          }
          if (tc.id) toolCalls[tc.index].id = tc.id;
          if (tc.function?.name)
            toolCalls[tc.index].function.name += tc.function.name;
          if (tc.function?.arguments)
            toolCalls[tc.index].function.arguments += tc.function.arguments;
        }
      }

      // Stream content tokens to client
      if (delta?.content) {
        content += delta.content;
        res.write(`data: ${JSON.stringify({ type: "token", content: delta.content })}\n\n`);
      }
    }

    // If the model made tool calls, execute them and continue
    if (finishReason === "tool_calls" && toolCalls.length > 0) {
      allMessages.push({
        role: "assistant",
        content: null,
        tool_calls: toolCalls.map((tc) => ({
          id: tc.id,
          type: "function" as const,
          function: tc.function,
        })),
      });

      for (const tc of toolCalls) {
        const args = JSON.parse(tc.function.arguments);
        res.write(
          `data: ${JSON.stringify({ type: "tool", name: tc.function.name, args })}\n\n`
        );
        const result = await callTool(tc.function.name, args);
        allMessages.push({
          role: "tool",
          tool_call_id: tc.id,
          content: JSON.stringify(result),
        });
      }
      continue; // Loop again with tool results
    }

    break; // No more tool calls — we're done
  }

  res.write(`data: ${JSON.stringify({ type: "done" })}\n\n`);
  res.end();
});

app.listen(3000, () => {
  console.log("Chatbot running at http://localhost:3000");
});
```

### Step 2: Frontend — Chat UI

```html
<!-- public/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Knowledge Base Chat</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #0a0a0f; color: #e4e4ec;
            display: flex; flex-direction: column; height: 100vh;
        }
        header {
            padding: 16px 24px; border-bottom: 1px solid #1e1e2e;
            font-weight: 700; font-size: 18px;
        }
        .messages {
            flex: 1; overflow-y: auto; padding: 24px;
            display: flex; flex-direction: column; gap: 16px;
        }
        .msg { max-width: 720px; line-height: 1.7; font-size: 15px; }
        .msg.user {
            align-self: flex-end; background: #1a2744;
            padding: 12px 16px; border-radius: 12px 12px 4px 12px;
        }
        .msg.assistant { align-self: flex-start; }
        .msg.tool {
            font-size: 12px; color: #55556a; font-family: monospace;
            padding: 4px 8px; background: #12121a; border-radius: 6px;
        }
        .msg a { color: #4f8fff; }
        .msg code {
            background: #12121a; padding: 2px 6px;
            border-radius: 4px; font-size: 0.9em; color: #6ba3ff;
        }
        .msg pre {
            background: #12121a; padding: 12px; border-radius: 8px;
            overflow-x: auto; margin: 8px 0; font-size: 13px;
        }
        .input-area {
            padding: 16px 24px; border-top: 1px solid #1e1e2e;
            display: flex; gap: 12px;
        }
        input {
            flex: 1; padding: 12px 16px; border-radius: 8px;
            background: #12121a; border: 1px solid #1e1e2e;
            color: #e4e4ec; font-size: 15px; outline: none;
        }
        input:focus { border-color: #4f8fff; }
        button {
            padding: 12px 24px; border-radius: 8px; border: none;
            background: #4f8fff; color: white; font-weight: 600;
            cursor: pointer; font-size: 15px;
        }
        button:hover { background: #6ba3ff; }
        button:disabled { opacity: 0.5; cursor: not-allowed; }
    </style>
</head>
<body>
    <header>⚡ Knowledge Base Chat</header>
    <div class="messages" id="messages"></div>
    <div class="input-area">
        <input type="text" id="input" placeholder="Ask about your codebase..."
               autofocus onkeydown="if(event.key==='Enter') send()">
        <button onclick="send()" id="send-btn">Send</button>
    </div>
    <script>
    const messages = [];
    const messagesEl = document.getElementById('messages');
    const inputEl = document.getElementById('input');
    const sendBtn = document.getElementById('send-btn');

    async function send() {
        const text = inputEl.value.trim();
        if (!text) return;

        inputEl.value = '';
        sendBtn.disabled = true;

        // Show user message
        messages.push({ role: 'user', content: text });
        addMessage('user', text);

        // Create assistant message container
        const assistantEl = addMessage('assistant', '');

        // Stream response
        const resp = await fetch('/api/chat', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ messages }),
        });

        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let fullContent = '';

        while (true) {
            const { done, value } = await reader.read();
            if (done) break;

            const text = decoder.decode(value);
            for (const line of text.split('\n')) {
                if (!line.startsWith('data: ')) continue;
                const data = JSON.parse(line.slice(6));

                if (data.type === 'token') {
                    fullContent += data.content;
                    assistantEl.innerHTML = renderMarkdown(fullContent);
                    messagesEl.scrollTop = messagesEl.scrollHeight;
                } else if (data.type === 'tool') {
                    addMessage('tool', `🔧 ${data.name}(${JSON.stringify(data.args).slice(0, 60)}...)`);
                }
            }
        }

        messages.push({ role: 'assistant', content: fullContent });
        sendBtn.disabled = false;
        inputEl.focus();
    }

    function addMessage(role, content) {
        const el = document.createElement('div');
        el.className = `msg ${role}`;
        el.innerHTML = renderMarkdown(content);
        messagesEl.appendChild(el);
        messagesEl.scrollTop = messagesEl.scrollHeight;
        return el;
    }

    function renderMarkdown(text) {
        return text
            .replace(/```(\w+)?\n([\s\S]*?)```/g, '<pre><code>$2</code></pre>')
            .replace(/`([^`]+)`/g, '<code>$1</code>')
            .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
            .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank">$1</a>')
            .replace(/\n/g, '<br>');
    }
    </script>
</body>
</html>
```

### Step 3: Run it

```bash
$ npx tsx server.ts
Chatbot running at http://localhost:3000
```

Open `http://localhost:3000` and start chatting with your knowledge base.

### Extending the chatbot

**Add authentication:**

```typescript
// Protect with a simple API key
app.use("/api", (req, res, next) => {
  const key = req.headers["x-api-key"];
  if (key !== process.env.CHAT_API_KEY) {
    return res.status(401).json({ error: "Unauthorized" });
  }
  next();
});
```

**Use a different LLM provider:**

```typescript
// Ollama (local)
const openai = new OpenAI({
  baseURL: "http://localhost:11434/v1",
  apiKey: "ollama",
});

// Anthropic via OpenAI-compatible proxy
const openai = new OpenAI({
  baseURL: "https://api.anthropic.com/v1",
  apiKey: process.env.ANTHROPIC_API_KEY,
});

// Azure OpenAI
const openai = new OpenAI({
  baseURL: "https://your-resource.openai.azure.com/openai/deployments/gpt-4o",
  apiKey: process.env.AZURE_OPENAI_KEY,
  defaultHeaders: { "api-key": process.env.AZURE_OPENAI_KEY },
});
```

**Add conversation persistence:**

```typescript
// Save conversations to SQLite (or any store)
import Database from "better-sqlite3";

const db = new Database("chat.db");
db.exec(`CREATE TABLE IF NOT EXISTS conversations (
  id TEXT PRIMARY KEY,
  messages TEXT,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP
)`);

app.post("/api/save", (req, res) => {
  const { id, messages } = req.body;
  db.prepare("INSERT OR REPLACE INTO conversations (id, messages) VALUES (?, ?)")
    .run(id, JSON.stringify(messages));
  res.json({ ok: true });
});
```

### What's next?

- Deploy with Docker (see [Deployment](@/docs/reference/deployment.md))
- Add Lua tools for write operations — let the chatbot create tickets, post to Slack
- Build a mobile-responsive version
- Add file upload for indexing new documents on the fly

