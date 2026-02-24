+++
title = "Recipe: Index Your GitHub Issues"
description = "A Lua connector that pulls issues from any GitHub repo and makes them searchable by your AI tools. Great for bug triage and project context."
date = 2026-02-21

[taxonomies]
tags = ["recipe", "lua"]
+++

Your AI coding assistant doesn't know about your GitHub issues. When you ask *"Are there any open bugs related to auth?"* it can only guess. With a short Lua connector, you can index every issue (with comments) and make them part of your AI's context.

### What you get

After setup, you can:
- Search issues from Cursor: *"Find issues about memory leaks"*
- Ask for context: *"What bugs were filed against the payment service?"*
- Triage smarter: *"Are there duplicates of this error message?"*

### The connector

Create `connectors/github-issues.lua`:

```lua
connector = {}
connector.name = "github-issues"
connector.description = "Index GitHub issues with comments from a repository"

function connector.scan(config)
    local owner = config.owner or error("config.owner is required")
    local repo = config.repo or error("config.repo is required")
    local token = config.token or env.get("GITHUB_TOKEN") or ""
    local state = config.state or "all"
    local max_pages = tonumber(config.max_pages) or 10

    local headers = {
        ["Accept"] = "application/vnd.github+json",
        ["User-Agent"] = "context-harness",
    }
    if token ~= "" then
        headers["Authorization"] = "Bearer " .. token
    end

    local items = {}
    local base = "https://api.github.com/repos/" .. owner .. "/" .. repo

    for page = 1, max_pages do
        local url = string.format(
            "%s/issues?state=%s&per_page=100&page=%d&sort=updated&direction=desc",
            base, state, page
        )

        local resp = http.get(url, { headers = headers })
        if resp.status ~= 200 then
            log.error("GitHub API error: " .. resp.status .. " — " .. resp.body)
            break
        end

        local issues = json.decode(resp.body)
        if #issues == 0 then break end

        for _, issue in ipairs(issues) do
            -- Skip pull requests (they show up in the issues endpoint)
            if not issue.pull_request then
                local labels = {}
                for _, l in ipairs(issue.labels or {}) do
                    table.insert(labels, l.name)
                end

                -- Build body with issue description
                local body = issue.body or "(no description)"

                -- Fetch comments if any
                if issue.comments and issue.comments > 0 then
                    local c_resp = http.get(
                        issue.comments_url,
                        { headers = headers }
                    )
                    if c_resp.status == 200 then
                        local comments = json.decode(c_resp.body)
                        for _, c in ipairs(comments) do
                            body = body .. string.format(
                                "\n\n---\n**%s** (%s):\n%s",
                                c.user and c.user.login or "unknown",
                                c.created_at or "",
                                c.body or ""
                            )
                        end
                    end
                end

                table.insert(items, {
                    id = owner .. "/" .. repo .. "#" .. issue.number,
                    title = string.format("#%d: %s", issue.number, issue.title),
                    body = body,
                    url = issue.html_url,
                    metadata = {
                        state = issue.state,
                        author = issue.user and issue.user.login or "unknown",
                        labels = table.concat(labels, ", "),
                        created = issue.created_at,
                        updated = issue.updated_at,
                    },
                })
            end
        end

        log.info(string.format("Page %d: fetched %d issues", page, #issues))
    end

    log.info(string.format("Total: %d issues from %s/%s", #items, owner, repo))
    return items
end

return connector
```

### Configure it

Add to `ctx.toml`:

```toml
[connectors.script.github-issues]
path = "connectors/github-issues.lua"
owner = "your-org"
repo = "your-repo"
token = "${GITHUB_TOKEN}"
state = "all"          # "open", "closed", or "all"
max_pages = 20         # 100 issues per page, 20 pages = 2000 issues
```

For private repos, create a [GitHub personal access token](https://github.com/settings/tokens) with `repo` scope and set it as an environment variable:

```bash
export GITHUB_TOKEN="ghp_..."
```

### Sync and search

```bash
$ ctx sync script:github-issues
sync script:github-issues
  Page 1: fetched 100 issues
  Page 2: fetched 100 issues
  Page 3: fetched 78 issues
  Total: 264 issues from your-org/your-repo
  fetched: 264 items
  upserted documents: 264
  chunks written: 1,203
ok

$ ctx search "authentication timeout" --source script:github-issues
1. [0.89] script:github-issues / #342: Auth tokens expire during long-running jobs
   "Users report 401 errors after ~30 minutes. The refresh token..."
2. [0.76] script:github-issues / #298: SSO session timeout too aggressive
   "When using SAML SSO, sessions expire after 15 minutes..."
```

### Multiple repos

Index issues from your whole org:

```toml
[connectors.script.platform-issues]
path = "connectors/github-issues.lua"
owner = "acme"
repo = "platform"
token = "${GITHUB_TOKEN}"

[connectors.script.infra-issues]
path = "connectors/github-issues.lua"
owner = "acme"
repo = "infrastructure"
token = "${GITHUB_TOKEN}"
state = "open"
max_pages = 5
```

```bash
$ ctx sync all
# Syncs both repos in parallel
```

Now you can search across all repos:

```bash
$ ctx search "database migration"
# Returns issues from both platform and infrastructure
```

Or filter to one:

```bash
$ ctx search "database migration" --source script:platform-issues
```

### Keep it fresh

Set up a cron job to re-sync periodically:

```bash
# Every 2 hours
0 */2 * * * cd ~/ctx-workspace && ctx sync script:github-issues
```

Or trigger a sync from a GitHub webhook after new issues are created.

### Rate limits

The GitHub API allows 5,000 requests/hour with a token (60/hour without). Each page of issues costs 1 request, plus 1 request per issue with comments. For a repo with 500 issues where 200 have comments, that's about 205 requests — well within limits.

Add `sleep(0.1)` after each `http.get` call if you're indexing very large repos and want to be extra cautious.
