--[[
  Context Harness Connector: GitHub Issues

  Fetches open issues from a public GitHub repository using the
  GitHub REST API. No authentication required for public repos.

  Configuration (add to ctx.toml):

    [connectors.script.github-issues]
    path = "examples/connectors/github-issues.lua"
    owner = "rust-lang"
    repo  = "rust"
    state = "open"          # open, closed, all
    labels = ""             # comma-separated filter
    per_page = "30"         # items per page (max 100)
    max_pages = "3"         # limit pagination

  Sync:
    ctx sync script:github-issues

  Test:
    ctx connector test examples/connectors/github-issues.lua --source github-issues
]]

connector = {
    name = "github-issues",
    version = "0.1.0",
    description = "Ingest issues from a public GitHub repository",
}

--- Scan GitHub Issues via the REST API.
--- @param config table Configuration from ctx.toml
--- @return table Array of source item tables
function connector.scan(config)
    local owner = config.owner or "octocat"
    local repo  = config.repo  or "Hello-World"
    local state = config.state or "open"
    local per_page = tonumber(config.per_page) or 30
    local max_pages = tonumber(config.max_pages) or 3

    local items = {}
    local base = "https://api.github.com"

    for page = 1, max_pages do
        local url = base .. "/repos/" .. owner .. "/" .. repo .. "/issues"
        local params = {
            state = state,
            per_page = tostring(per_page),
            page = tostring(page),
            sort = "updated",
            direction = "desc",
        }

        if config.labels and config.labels ~= "" then
            params.labels = config.labels
        end

        log.info("Fetching page " .. page .. " from " .. owner .. "/" .. repo)

        local resp = http.get(url, {
            headers = {
                ["Accept"] = "application/vnd.github+json",
                ["User-Agent"] = "context-harness-connector",
            },
            params = params,
        })

        if not resp.ok then
            log.error("GitHub API error: HTTP " .. resp.status)
            break
        end

        if resp.json == nil or #resp.json == 0 then
            log.info("No more issues on page " .. page)
            break
        end

        for _, issue in ipairs(resp.json) do
            -- Skip pull requests (they appear in the issues endpoint)
            if issue.pull_request == nil then
                -- Build the body from title + labels + body text
                local body_parts = { "# " .. (issue.title or "Untitled") }

                if issue.labels and #issue.labels > 0 then
                    local label_names = {}
                    for _, label in ipairs(issue.labels) do
                        table.insert(label_names, label.name)
                    end
                    table.insert(body_parts, "Labels: " .. table.concat(label_names, ", "))
                end

                if issue.body and issue.body ~= "" then
                    table.insert(body_parts, "")
                    table.insert(body_parts, issue.body)
                end

                table.insert(items, {
                    source_id = tostring(issue.number),
                    title = issue.title,
                    body = table.concat(body_parts, "\n"),
                    author = issue.user and issue.user.login or nil,
                    source_url = issue.html_url,
                    content_type = "text/markdown",
                    created_at = issue.created_at,
                    updated_at = issue.updated_at,
                    metadata_json = json.encode({
                        number = issue.number,
                        state = issue.state,
                        comments = issue.comments,
                    }),
                })
            end
        end

        -- Respect rate limits
        local remaining = resp.headers["x-ratelimit-remaining"]
        if remaining and tonumber(remaining) < 5 then
            log.warn("Rate limit low (" .. remaining .. " remaining), stopping early")
            break
        end
    end

    log.info("Fetched " .. #items .. " issues from " .. owner .. "/" .. repo)
    return items
end

