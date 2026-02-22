+++
title = "Lua Connectors"
description = "Write custom data source connectors in Lua — no Rust compilation needed."
weight = 2
+++

Lua scripted connectors let you add *any* data source to Context Harness by writing a simple Lua script. The script runs in a sandboxed Lua 5.4 VM with access to HTTP, JSON, filesystem, and other host APIs. No Rust compilation needed — just write a `.lua` file and sync.

### How it works

1. Write a `.lua` file that implements `connector.scan(config) → items[]`
2. Add it to `ctx.toml` under `[connectors.script.<name>]`
3. Run `ctx sync script:<name>` to ingest

### Quick example: GitHub Issues

```lua
connector = {
    name = "github-issues",
    version = "0.1.0",
    description = "Ingest GitHub issues as searchable documents",
}

function connector.scan(config)
    local items = {}
    local page = 1

    while true do
        local url = string.format(
            "https://api.github.com/repos/%s/%s/issues?state=all&per_page=100&page=%d",
            config.owner, config.repo, page
        )

        local resp = http.get(url, {
            headers = {
                ["Authorization"] = "Bearer " .. config.token,
                ["Accept"] = "application/vnd.github.v3+json",
            },
        })

        if resp.status ~= 200 then
            log.error("GitHub API returned " .. resp.status)
            break
        end

        local issues = json.decode(resp.body)
        if #issues == 0 then break end

        for _, issue in ipairs(issues) do
            local body = "# " .. issue.title .. "\n\n"
            body = body .. (issue.body or "") .. "\n\n"
            body = body .. "**State:** " .. issue.state .. "\n"

            if issue.labels then
                local label_names = {}
                for _, l in ipairs(issue.labels) do
                    table.insert(label_names, l.name)
                end
                body = body .. "**Labels:** " .. table.concat(label_names, ", ") .. "\n"
            end

            table.insert(items, {
                source_id  = tostring(issue.number),
                title      = "[#" .. issue.number .. "] " .. issue.title,
                body       = body,
                author     = issue.user.login,
                created_at = issue.created_at,
                updated_at = issue.updated_at,
                source_url = issue.html_url,
                metadata   = { state = issue.state },
            })
        end

        page = page + 1
        sleep(0.5)  -- Rate limiting
    end

    log.info("Fetched " .. #items .. " issues")
    return items
end
```

```toml
[connectors.script.github-issues]
path = "connectors/github-issues.lua"
timeout = 60
owner = "parallax-labs"
repo = "context-harness"
token = "${GITHUB_TOKEN}"
```

### Example: Confluence wiki

```lua
connector = {
    name = "confluence",
    version = "0.1.0",
    description = "Ingest Confluence pages as searchable documents",
}

function connector.scan(config)
    local items = {}
    local start = 0
    local limit = 50

    while true do
        local url = string.format(
            "%s/rest/api/content?spaceKey=%s&expand=body.storage,version&start=%d&limit=%d",
            config.url, config.space, start, limit
        )

        local resp = http.get(url, {
            headers = {
                ["Authorization"] = "Basic " ..
                    base64.encode(config.user .. ":" .. config.token),
                ["Accept"] = "application/json",
            },
        })

        if resp.status ~= 200 then
            log.error("Confluence API error: " .. resp.status)
            break
        end

        local data = json.decode(resp.body)
        for _, page in ipairs(data.results) do
            -- Strip HTML tags for plain text
            local body = page.body.storage.value:gsub("<[^>]+>", "")

            table.insert(items, {
                source_id  = page.id,
                title      = page.title,
                body       = body,
                updated_at = page.version.when,
                source_url = config.url .. page._links.webui,
                metadata   = { space = config.space },
            })
        end

        if data.size < limit then break end
        start = start + limit
        sleep(0.3)
    end

    log.info("Fetched " .. #items .. " Confluence pages")
    return items
end
```

```toml
[connectors.script.wiki]
path = "connectors/confluence.lua"
timeout = 120
url = "https://mycompany.atlassian.net/wiki"
space = "ENG"
user = "me@company.com"
token = "${CONFLUENCE_API_TOKEN}"
```

### Example: Notion database

```lua
connector = {
    name = "notion",
    version = "0.1.0",
    description = "Ingest Notion database entries",
}

function connector.scan(config)
    local items = {}
    local has_more = true
    local cursor = nil

    while has_more do
        local body = { page_size = 100 }
        if cursor then body.start_cursor = cursor end

        local resp = http.post(
            "https://api.notion.com/v1/databases/" .. config.database_id .. "/query",
            json.encode(body),
            {
                headers = {
                    ["Authorization"] = "Bearer " .. config.token,
                    ["Notion-Version"] = "2022-06-28",
                    ["Content-Type"] = "application/json",
                },
            }
        )

        local data = json.decode(resp.body)
        for _, page in ipairs(data.results) do
            -- Extract title from Name property
            local title = "Untitled"
            if page.properties.Name and page.properties.Name.title then
                local parts = {}
                for _, t in ipairs(page.properties.Name.title) do
                    table.insert(parts, t.plain_text)
                end
                title = table.concat(parts)
            end

            -- Fetch page content (blocks)
            local blocks_resp = http.get(
                "https://api.notion.com/v1/blocks/" .. page.id .. "/children?page_size=100",
                {
                    headers = {
                        ["Authorization"] = "Bearer " .. config.token,
                        ["Notion-Version"] = "2022-06-28",
                    },
                }
            )

            local body_text = ""
            if blocks_resp.status == 200 then
                local blocks = json.decode(blocks_resp.body)
                for _, block in ipairs(blocks.results) do
                    if block.paragraph and block.paragraph.rich_text then
                        for _, rt in ipairs(block.paragraph.rich_text) do
                            body_text = body_text .. rt.plain_text
                        end
                        body_text = body_text .. "\n\n"
                    end
                end
            end

            table.insert(items, {
                source_id  = page.id,
                title      = title,
                body       = body_text,
                updated_at = page.last_edited_time,
                source_url = page.url,
            })

            sleep(0.35)  -- Notion rate limit: 3 req/s
        end

        has_more = data.has_more
        cursor = data.next_cursor
    end

    log.info("Fetched " .. #items .. " Notion pages")
    return items
end
```

```toml
[connectors.script.notion]
path = "connectors/notion.lua"
timeout = 300
database_id = "abc123..."
token = "${NOTION_API_KEY}"
```

### Example: Slack channel history

```lua
connector = {
    name = "slack",
    version = "0.1.0",
    description = "Ingest Slack channel messages",
}

function connector.scan(config)
    local items = {}

    for _, channel_id in ipairs({config.channel_id}) do
        local cursor = nil
        repeat
            local url = "https://slack.com/api/conversations.history?channel="
                .. channel_id .. "&limit=200"
            if cursor then url = url .. "&cursor=" .. cursor end

            local resp = http.get(url, {
                headers = { ["Authorization"] = "Bearer " .. config.token },
            })

            local data = json.decode(resp.body)
            if not data.ok then
                log.error("Slack API error: " .. (data.error or "unknown"))
                break
            end

            for _, msg in ipairs(data.messages) do
                if msg.text and #msg.text > 20 then  -- Skip very short messages
                    table.insert(items, {
                        source_id  = msg.ts,
                        title      = msg.text:sub(1, 80),
                        body       = msg.text,
                        author     = msg.user or "bot",
                        created_at = os.date("!%Y-%m-%dT%H:%M:%SZ", tonumber(msg.ts)),
                        metadata   = { channel = channel_id },
                    })
                end
            end

            cursor = data.response_metadata and data.response_metadata.next_cursor
            if cursor == "" then cursor = nil end
            sleep(1.2)  -- Slack tier 3 rate limit
        until cursor == nil
    end

    log.info("Fetched " .. #items .. " messages")
    return items
end
```

```toml
[connectors.script.slack]
path = "connectors/slack.lua"
timeout = 120
channel_id = "C01ABCDEF"
token = "${SLACK_BOT_TOKEN}"
```

### Script contract

Every connector script must define:

- `connector.name` — identifier string
- `connector.version` — semver string
- `connector.description` — human-readable description
- `connector.scan(config)` — function returning an array of items

Each returned item can have:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `source_id` | string | **yes** | Unique ID within this source |
| `body` | string | **yes** | Document content (markdown, text, etc.) |
| `title` | string | no | Document title |
| `author` | string | no | Author name |
| `created_at` | string | no | ISO 8601 timestamp |
| `updated_at` | string | no | ISO 8601 timestamp |
| `source_url` | string | no | Web URL for viewing the original |
| `content_type` | string | no | MIME type (default: `text/plain`) |
| `metadata` | table | no | Arbitrary key-value pairs |

### Configuration

```toml
[connectors.script.my-source]
path = "connectors/my-source.lua"  # Path to Lua script (required)
timeout = 30                       # Execution timeout in seconds (default: 30)
# All other keys become config.* in Lua
api_url = "https://api.example.com"
api_key = "${MY_API_KEY}"          # ${VAR} expands from env
```

### Host APIs available in scripts

| API | Functions | Example |
|-----|-----------|---------|
| **`http`** | `get`, `post`, `put` | `http.get(url, {headers={...}})` → `{status, body, headers}` |
| **`json`** | `encode`, `decode` | `json.decode('{"key":"val"}')` → table |
| **`env`** | `get` | `env.get("API_KEY")` → string |
| **`log`** | `info`, `warn`, `error`, `debug` | `log.info("Processing...")` |
| **`fs`** | `read`, `list` | `fs.read("path/to/file")` → string |
| **`base64`** | `encode`, `decode` | `base64.encode("hello")` → `"aGVsbG8="` |
| **`crypto`** | `sha256`, `hmac_sha256` | `crypto.sha256("data")` → hex string |
| **`sleep`** | (global) | `sleep(1.5)` — pause 1.5 seconds |

### CLI commands

```bash
# Scaffold a new connector with a template:
$ ctx connector init my-source
Created connectors/my-source.lua

# Test without modifying the database:
$ ctx connector test connectors/my-source.lua
scan() returned 42 items:
  [0] source_id="item-001" title="First item"
  [1] source_id="item-002" title="Second item"
  ...

# Test with config from ctx.toml:
$ ctx connector test connectors/jira.lua --source jira

# Sync:
$ ctx sync script:my-source
sync script:my-source
  fetched: 42 items
  upserted documents: 42
  chunks written: 187
ok
```
