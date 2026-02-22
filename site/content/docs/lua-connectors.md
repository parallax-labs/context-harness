+++
title = "Lua Scripted Connectors"
description = "Write custom data source connectors in Lua — no Rust compilation needed."
weight = 5

[extra]
sidebar_label = "Lua Connectors"
sidebar_group = "Extensibility"
sidebar_order = 5
+++

Lua scripted connectors let you add any data source to Context Harness by writing a simple Lua script. The script runs in a sandboxed Lua 5.4 VM with access to HTTP, JSON, filesystem, and other host APIs. No Rust compilation needed.

### How it works

1. Write a `.lua` file that implements `connector.scan(config) → items[]`
2. Add it to `ctx.toml` under `[connectors.script.<name>]`
3. Run `ctx sync script:<name>` to ingest

### Example: GitHub Issues

Here's a complete connector that ingests GitHub issues:

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
            -- Build a rich body with labels and comments
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
                metadata   = {
                    state  = issue.state,
                    labels = table.concat(label_names or {}, ","),
                },
            })
        end

        page = page + 1
        sleep(0.5)  -- Rate limiting
    end

    log.info("Fetched " .. #items .. " issues")
    return items
end
```

### Configuration

```toml
[connectors.script.github-issues]
path = "connectors/github-issues.lua"
timeout = 60
owner = "parallax-labs"
repo = "context-harness"
token = "${GITHUB_TOKEN}"
```

All keys besides `path` and `timeout` become the `config` table passed to `connector.scan()`. Values with `${VAR}` are expanded from environment variables.

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

# Test it without modifying the database:
$ ctx connector test connectors/my-source.lua

# Test with config from ctx.toml:
$ ctx connector test connectors/jira.lua --source jira

# Sync it:
$ ctx sync script:my-source
```
