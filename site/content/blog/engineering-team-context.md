+++
title = "Recipe: Unified Context for Your Engineering Team"
description = "Set up a shared Context Harness instance that indexes multiple repos, Jira, runbooks, and ADRs — so every engineer's AI assistant knows your whole stack."
date = 2026-02-20

[taxonomies]
tags = ["recipe", "teams"]
+++

Most AI coding assistants only see the file you have open. That's fine for autocomplete, but useless when you ask *"How does the auth service validate tokens?"* and the answer lives in a different repo, a Confluence page, or a Jira ticket from three months ago.

This recipe sets up a shared Context Harness instance that gives every engineer's AI assistant access to your entire engineering org's knowledge — across repos, issue trackers, and internal docs.

### What you'll build

```
┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│ git:platform │ │  git:infra   │ │ s3:runbooks  │ │ script:jira  │
└──────┬───────┘ └──────┬───────┘ └──────┬───────┘ └──────┬───────┘
       │                │                │                │
       └────────────────┴────────────────┴────────────────┘
                                │
                    ┌───────────▼───────────┐
                    │  ctx serve mcp        │
                    │  :7331/mcp            │
                    └───────────┬───────────┘
                                │
          ┌─────────────────────┼─────────────────────┐
          │                     │                     │
    ┌─────▼─────┐        ┌─────▼─────┐        ┌─────▼─────┐
    │  Alice's  │        │  Bob's    │        │ Carol's   │
    │  Cursor   │        │  Cursor   │        │  Claude   │
    └───────────┘        └───────────┘        └───────────┘
```

One server. Every engineer's AI gets the same context. No per-person setup beyond a one-line MCP config.

### The config

Create a dedicated directory for your team's context:

```bash
mkdir -p ~/team-context/config ~/team-context/connectors
```

`~/team-context/config/ctx.toml`:

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dims = 1536
batch_size = 64

[retrieval]
final_limit = 15
hybrid_alpha = 0.6

[server]
bind = "127.0.0.1:7331"

# ── Source code repos ─────────────────────────────────────────

[connectors.git.platform]
url = "https://github.com/acme/platform.git"
branch = "main"
include_globs = ["docs/**/*.md", "src/**/*.rs", "README.md", "CHANGELOG.md", "ADR/**/*.md"]
exclude_globs = ["**/target/**"]
shallow = true

[connectors.git.auth-service]
url = "https://github.com/acme/auth-service.git"
branch = "main"
include_globs = ["src/**/*.go", "docs/**/*.md", "README.md"]
shallow = true

[connectors.git.frontend]
url = "https://github.com/acme/web-app.git"
branch = "main"
include_globs = ["src/**/*.tsx", "docs/**/*.md", "README.md"]
exclude_globs = ["**/node_modules/**", "**/__tests__/**"]
shallow = true

# ── Runbooks from S3 ─────────────────────────────────────────

[connectors.s3.runbooks]
bucket = "acme-engineering"
prefix = "runbooks/"
region = "us-east-1"
include_globs = ["**/*.md"]

# ── Jira issues ──────────────────────────────────────────────

[connectors.script.jira]
path = "connectors/jira.lua"
timeout = 120
url = "https://acme.atlassian.net"
project = "ENG"
api_token = "${JIRA_API_TOKEN}"
max_results = 500

# ── GitHub Issues ────────────────────────────────────────────

[connectors.script.github-issues]
path = "connectors/github-issues.lua"
owner = "acme"
repo = "platform"
token = "${GITHUB_TOKEN}"
state = "all"
max_pages = 10

# ── Agents ───────────────────────────────────────────────────

[agents.inline.code-reviewer]
description = "Reviews code against project conventions and patterns"
tools = ["search", "get"]
system_prompt = """
You are a senior code reviewer for the Acme engineering team. When
reviewing code:
1. Search for relevant coding conventions, patterns, and ADRs
2. Cite specific documents when suggesting changes
3. Focus on correctness, consistency with existing patterns, and
   potential issues — not style nitpicks
"""

[agents.inline.oncall]
description = "Helps triage production incidents using runbooks and past issues"
tools = ["search", "get"]
system_prompt = """
You are an on-call assistant for the Acme engineering team. When an
incident is reported:
1. Search runbooks for relevant procedures
2. Search past Jira issues for similar incidents
3. Provide step-by-step remediation guidance
4. Cite runbook titles and issue numbers
Be systematic: gather context, identify the issue, recommend actions.
"""
```

### Initial setup

```bash
cd ~/team-context

# Initialize the database
ctx init

# Sync all sources (runs in parallel)
ctx sync all

# Generate embeddings for hybrid search
ctx embed pending

# Start the server
ctx serve mcp
```

The first sync clones repos and fetches all issues — expect 2-5 minutes depending on repo sizes. Subsequent syncs are incremental and much faster.

### Connect every engineer

Each engineer adds one file to their Cursor workspace — `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "acme-context": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

Commit this to each repo so everyone gets it automatically. If the server runs on a shared machine, use that machine's IP instead of `127.0.0.1`.

### What engineers can do now

**Find code patterns across repos:**
- *"How does the auth service validate JWT tokens?"*
- *"Show me examples of error handling in the platform repo"*
- *"What patterns do we use for database migrations?"*

**Get context from issues:**
- *"Are there any known bugs with the payment flow?"*
- *"What was the resolution for the auth outage last month?"*
- *"Find Jira tickets related to performance regression"*

**Use runbooks during incidents:**
- *"What's the procedure for a database failover?"*
- *"How do I roll back a deployment on the auth service?"*

**Review code with team conventions:**
- Select the *code-reviewer* agent and paste a diff
- It searches for relevant ADRs and coding standards, then reviews against them

### Keep it fresh

Add a cron job to sync every 2 hours:

```bash
# crontab -e
0 */2 * * * cd ~/team-context && ctx sync all && ctx embed pending >> /var/log/ctx-sync.log 2>&1
```

Or run it as a systemd service that syncs on a timer:

```ini
# /etc/systemd/system/ctx-sync.timer
[Unit]
Description=Context Harness sync timer

[Timer]
OnCalendar=*:0/120
Persistent=true

[Install]
WantedBy=timers.target
```

### Scaling tips

- **Large repos:** Use `shallow = true` and targeted `include_globs` to avoid indexing test fixtures, generated code, and vendored dependencies.
- **Many Jira issues:** Set `max_results` to control how many issues to index. Start with recent issues (500-1000) and expand if needed.
- **Multiple teams:** Run separate Context Harness instances on different ports with different configs. Each team gets their own context scoped to their repos and projects.
- **CI-built index:** Build the SQLite database in CI and distribute it as an artifact. Engineers download the pre-built database and just run `ctx serve mcp` — no API keys needed locally.

### The payoff

Before: engineer asks AI about auth, gets a generic answer about JWT.
After: engineer asks AI about auth, gets the *team's actual auth implementation* with links to the relevant code, ADRs, and related Jira issues.

Context Harness turns your AI assistant from a general-purpose tool into a team member that actually knows your codebase.
