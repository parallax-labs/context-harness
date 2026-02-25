+++
title = "Available Extensions"
description = "Complete catalog of connectors, tools, and agents in the community registry."
weight = 3
+++

The [community registry](https://github.com/parallax-labs/ctx-registry) ships with 16 production-ready extensions. All are Lua scripts that use the built-in host APIs — no external dependencies required.

---

## Connectors

Connectors ingest data from external sources into your knowledge base. Add them to `ctx.toml` via `ctx registry add`, fill in credentials, and run `ctx sync`.

### No authentication required

These connectors work immediately with public APIs:

#### rss

Ingest articles from any RSS 2.0 or Atom feed. Parses XML with Lua patterns — handles CDATA sections and HTML content.

```toml
[connectors.script.rss]
path = "~/.ctx/registries/community/connectors/rss/connector.lua"
feed_url = "https://blog.rust-lang.org/feed.xml"
max_items = "50"
```

Multiple feeds? Add separate instances:

```toml
[connectors.script.rss-rust-blog]
path = "~/.ctx/registries/community/connectors/rss/connector.lua"
feed_url = "https://blog.rust-lang.org/feed.xml"

[connectors.script.rss-company-blog]
path = "~/.ctx/registries/community/connectors/rss/connector.lua"
feed_url = "https://engineering.mycompany.com/feed"
```

#### stackoverflow

Ingest Q&A from any Stack Exchange site. Fetches questions with accepted answers.

```toml
[connectors.script.stackoverflow]
path = "~/.ctx/registries/community/connectors/stackoverflow/connector.lua"
tagged = "rust"
site = "stackoverflow"
sort = "votes"
per_page = "30"
max_pages = "3"
```

#### devto

Ingest articles from Dev.to by tag or author. Fetches full markdown body.

```toml
[connectors.script.devto]
path = "~/.ctx/registries/community/connectors/devto/connector.lua"
tag = "rust"
per_page = "30"
max_pages = "3"
```

#### hackernews

Ingest stories from Hacker News — top, best, new, ask, or show feeds.

```toml
[connectors.script.hackernews]
path = "~/.ctx/registries/community/connectors/hackernews/connector.lua"
feed = "top"
max_items = "30"
fetch_comments = "true"
```

#### github-discussions

Ingest discussions from a GitHub repository. Token is optional for public repos.

```toml
[connectors.script.github-discussions]
path = "~/.ctx/registries/community/connectors/github-discussions/connector.lua"
owner = "rust-lang"
repo = "rust"
token = "${GITHUB_TOKEN}"
max_items = "50"
```

### Authentication required

These connectors need API tokens or credentials. Store secrets in environment variables using `${VAR}` syntax.

#### jira

Ingest issues from Jira Cloud with full ADF-to-markdown conversion and comments.

```toml
[connectors.script.jira]
path = "~/.ctx/registries/community/connectors/jira/connector.lua"
url = "https://mycompany.atlassian.net"
email = "bot@company.com"
api_token = "${JIRA_API_TOKEN}"
project = "ENG"
max_results = "100"
include_comments = "true"
```

#### confluence

Ingest pages from a Confluence Cloud space with HTML-to-markdown conversion.

```toml
[connectors.script.confluence]
path = "~/.ctx/registries/community/connectors/confluence/connector.lua"
url = "https://mycompany.atlassian.net"
email = "bot@company.com"
api_token = "${CONFLUENCE_API_TOKEN}"
space_key = "ENG"
max_pages = "100"
```

#### notion

Ingest pages from a Notion database with recursive block-level content extraction.

```toml
[connectors.script.notion]
path = "~/.ctx/registries/community/connectors/notion/connector.lua"
api_token = "${NOTION_API_TOKEN}"
database_id = "abc123..."
page_size = "50"
```

**Setup:** Create an integration at [notion.so/my-integrations](https://www.notion.so/my-integrations), then share the database with it.

#### slack

Ingest message history from Slack channels with threaded conversation grouping.

```toml
[connectors.script.slack]
path = "~/.ctx/registries/community/connectors/slack/connector.lua"
bot_token = "${SLACK_BOT_TOKEN}"
channel = "C0123456789"
days_back = "30"
max_messages = "500"
include_threads = "true"
```

**Setup:** Create a Slack App, add `channels:history`, `channels:read`, `users:read` scopes, install to workspace, invite bot to channel.

#### linear

Ingest issues from Linear with comments and labels.

```toml
[connectors.script.linear]
path = "~/.ctx/registries/community/connectors/linear/connector.lua"
api_key = "${LINEAR_API_KEY}"
max_issues = "100"
include_comments = "true"
```

---

## Tools

Tools are callable actions exposed via the MCP server. They auto-discover from registries — no config needed unless the tool requires credentials.

#### web-fetch

Fetch any URL and return clean extracted text. Strips HTML tags, scripts, and styles.

**Parameters:** `url` (required), `max_length` (default 10000)

```bash
# Auto-discovered — no config needed
# Agents can call it directly via MCP
```

#### create-jira-ticket

Create Jira tickets with optional knowledge-base enrichment. Searches the KB for related docs and appends them to the ticket description.

**Parameters:** `title`, `body`, `project`, `issue_type`, `priority`, `labels`, `enrich`

```toml
[tools.script.create-jira-ticket]
path = "~/.ctx/registries/community/tools/create-jira-ticket/tool.lua"
url = "https://mycompany.atlassian.net"
email = "bot@company.com"
api_token = "${JIRA_API_TOKEN}"
```

#### send-slack-message

Post messages to Slack channels with optional threading.

**Parameters:** `channel` (required), `text` (required), `thread_ts`

```toml
[tools.script.send-slack-message]
path = "~/.ctx/registries/community/tools/send-slack-message/tool.lua"
bot_token = "${SLACK_BOT_TOKEN}"
```

#### create-github-issue

Create issues in GitHub repositories with labels and assignees.

**Parameters:** `owner`, `repo`, `title`, `body`, `labels`, `assignees`

```toml
[tools.script.create-github-issue]
path = "~/.ctx/registries/community/tools/create-github-issue/tool.lua"
token = "${GITHUB_TOKEN}"
```

---

## Agents

Agents are personas with system prompts and tool access. They auto-discover from registries.

#### researcher

A research agent that searches the knowledge base with multiple query strategies and synthesizes cited answers. Pre-loads relevant context based on the user's topic.

**Arguments:** `topic`, `depth` (quick/standard/deep)

#### incident-responder

An incident response agent that surfaces relevant runbooks, past incidents, and architecture documentation. Categorizes search results into runbooks, past incidents, and architecture docs.

**Arguments:** `incident` (required), `severity` (sev1-sev4), `service`
