+++
title = "Usage Guide"
description = "Step-by-step guide to installing, browsing, and using community extensions."
weight = 2
+++

This guide walks you through the complete registry workflow — from first install to using community connectors and tools in production.

### 1. Install the community registry

Run `ctx registry init` to clone the community registry and add it to your config:

```bash
$ ctx registry init
Cloning community extension registry...
Installed: 10 connectors, 4 tools, 2 agents
Added [registries.community] to ./config/ctx.toml
Run `ctx registry list` to see available extensions.
```

This does two things:
- Clones the registry to `~/.ctx/registries/community/`
- Appends a `[registries.community]` section to your `ctx.toml`

The command is idempotent — running it again detects the existing clone and skips.

### 2. Browse available extensions

List everything in your configured registries:

```bash
$ ctx registry list
Registries:

  community — ~/.ctx/registries/community (git) [readonly]
    10 connectors, 4 tools, 2 agents

Available extensions:

  agents:
    incident-responder — Incident response agent [incident, ops, runbook, sre]
    researcher — Research agent with citations [research, search, synthesis]
  connectors:
    confluence — Confluence Cloud pages [confluence, atlassian, wiki]
    devto — Dev.to articles [devto, blog, articles]
    github-discussions — GitHub Discussions [github, discussions, q&a]
    hackernews — Hacker News stories [hackernews, news, tech]
    jira — Jira Cloud issues [jira, atlassian, issue-tracking]
    linear — Linear issues [linear, issue-tracking]
    notion — Notion database pages [notion, documentation, wiki]
    rss — RSS and Atom feeds [rss, atom, feed, blog, news]
    slack — Slack channel history [slack, messaging, chat]
    stackoverflow — Stack Exchange Q&A [stackoverflow, qa, reference]
  tools:
    create-github-issue — Create GitHub issues [github, issues]
    create-jira-ticket — Create Jira tickets [jira, atlassian]
    send-slack-message — Post Slack messages [slack, messaging]
    web-fetch — Fetch URL text content [web, fetch, scrape]
```

### 3. Search and inspect

Find extensions by name, tag, or description:

```bash
$ ctx registry search jira
Found 2 extensions matching 'jira':
  connectors/jira — Ingest issues from Jira Cloud [jira, atlassian]
  tools/create-jira-ticket — Create Jira tickets [jira, atlassian]
```

Get full details on a specific extension:

```bash
$ ctx registry info connectors/rss
Extension: connectors/rss
Registry:  community
Script:    ~/.ctx/registries/community/connectors/rss/connector.lua
Description: Ingest articles from RSS and Atom feeds
Tags: rss, atom, feed, blog, news
Required config: feed_url
Host APIs: http
```

### 4. Add a connector to your config

Connectors need credentials and configuration, so they require an explicit entry in `ctx.toml`. The `add` command scaffolds it for you:

```bash
$ ctx registry add connectors/rss
Added [connectors.script.rss] to ./config/ctx.toml
```

This appends a config block like:

```toml
[connectors.script.rss]
path = "~/.ctx/registries/community/connectors/rss/connector.lua"
feed_url = ""  # TODO: set this
```

Edit the config to fill in the required values:

```toml
[connectors.script.rss]
path = "~/.ctx/registries/community/connectors/rss/connector.lua"
feed_url = "https://blog.rust-lang.org/feed.xml"
max_items = "30"
```

Then sync:

```bash
$ ctx sync script:rss
sync script:rss
  fetched: 30 items
  upserted documents: 30
  chunks written: 87
ok
```

### 5. Tools and agents auto-discover

Unlike connectors, **tools and agents from registries are automatically available** when you start the MCP server. No config entry needed:

```bash
$ ctx serve mcp
Registered 7 tools:
  POST /tools/search — Search the knowledge base (builtin)
  POST /tools/get — Retrieve a document by UUID (builtin)
  POST /tools/sources — List connector status (builtin)
  POST /tools/web-fetch — Fetch URL text content (lua)
  POST /tools/create-jira-ticket — Create Jira tickets (lua)
  POST /tools/send-slack-message — Post Slack messages (lua)
  POST /tools/create-github-issue — Create GitHub issues (lua)
Registered 2 agents:
  POST /agents/researcher/prompt — Research agent (lua)
  POST /agents/incident-responder/prompt — Incident response (lua)
MCP server listening on http://127.0.0.1:7331
```

If a tool requires configuration (like `create-jira-ticket` needs Jira credentials), use `ctx registry add` to scaffold a config entry with the required fields.

### 6. Update registries

Pull the latest changes from all Git-backed registries:

```bash
$ ctx registry update
Updating registry 'community'...
Already up to date.
```

Or update a specific one:

```bash
$ ctx registry update --name community
```

### 7. Override an extension

If you want to customize a read-only community extension, use `override` to copy it to a writable registry:

```bash
$ ctx registry override connectors/rss
Copied connectors/rss to ~/.ctx/registries/personal/connectors/rss/
Edit the copy at: ~/.ctx/registries/personal/connectors/rss/connector.lua
```

The copy takes precedence over the original. You can modify it freely without affecting the community version or causing Git conflicts on the next `update`.

### 8. Project-local extensions

Create a `.ctx/` directory in your project root for project-specific extensions:

```
my-project/
  .ctx/
    connectors/
      internal-api/connector.lua
    tools/
      deploy/tool.lua
```

Context Harness automatically discovers `.ctx/` by walking up from the current directory (like `.git/` discovery). Project-local extensions have higher precedence than all registries and don't need a `registry.toml` — they're discovered by directory convention.

### 9. Multiple registries

You can configure multiple registries with different precedence levels:

```toml
[registries.community]
url = "https://github.com/parallax-labs/ctx-registry.git"
path = "~/.ctx/registries/community"
readonly = true
auto_update = true

[registries.company]
url = "git@github.com:mycompany/ctx-extensions.git"
path = "~/.ctx/registries/company"
readonly = true

[registries.personal]
path = "~/.ctx/registries/personal"
readonly = false
```

Later entries in the config have higher precedence. Extensions in `personal` override those in `company`, which override `community`.
