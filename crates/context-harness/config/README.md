# Config

- **`ctx.example.toml`** — Template with commented examples. Copy and customize for your own project.
- **`ctx.toml`** — Active config for developing context-harness (this repo). Used when you run `ctx` from the repo root without `--config`.

## Self-harness (building context-harness)

This config indexes:

| Connector   | Path            | Contents                    |
|------------|-----------------|-----------------------------|
| `docs`     | `./docs`        | Design, usage, AGENTS, etc. |
| `site`     | `./site/content`| Published docs + blog       |
| `root`     | `.`             | README, CONTRIBUTING, CHANGELOG |
| `src`      | `./src`         | Rust source                 |

**From repo root:**

```bash
ctx init          # create DB if needed
ctx sync all      # ingest all connectors
ctx serve         # HTTP + MCP on 127.0.0.1:7332
```

Point MCP or the Cursor extension at `http://127.0.0.1:7332`. The demo uses port 7331, so both can run at once.

**Embedding:** `provider = "local"` (all-minilm-l6-v2) so no API key is required. Override in a local file or set `OPENAI_API_KEY` and switch to `openai` in a copy if you prefer.
