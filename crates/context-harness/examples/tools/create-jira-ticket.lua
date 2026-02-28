--[[
  Context Harness Tool: create_jira_ticket

  Creates a Jira ticket with optional RAG-enriched description.
  If the body mentions technical concepts, the tool searches the
  knowledge base for related documentation and appends links.

  Configuration:
    [tools.script.create_jira_ticket]
    path = "tools/create-jira-ticket.lua"
    url = "https://mycompany.atlassian.net"
    email = "bot@company.com"
    api_token = "${JIRA_API_TOKEN}"

  Test:
    ctx tool test examples/tools/create-jira-ticket.lua \
      --param title="Fix auth bug" \
      --param body="The login flow breaks when..." \
      --source create_jira_ticket
]]

tool = {
    name = "create_jira_ticket",
    version = "1.0.0",
    description = "Create a Jira ticket, optionally enriched with related docs",
    parameters = {
        {
            name = "title",
            type = "string",
            required = true,
            description = "Ticket title",
        },
        {
            name = "body",
            type = "string",
            required = true,
            description = "Ticket description (markdown)",
        },
        {
            name = "project",
            type = "string",
            required = false,
            description = "Jira project key",
            default = "ENG",
        },
        {
            name = "priority",
            type = "string",
            required = false,
            description = "Priority",
            default = "Medium",
            enum = { "Lowest", "Low", "Medium", "High", "Highest" },
        },
        {
            name = "enrich",
            type = "boolean",
            required = false,
            description = "Search knowledge base and append related docs",
            default = true,
        },
    },
}

function tool.execute(params, context)
    local description = params.body

    -- Optionally enrich with related documentation
    if params.enrich then
        local results = context.search(params.title, {
            mode = "hybrid",
            limit = 3,
        })

        if #results > 0 then
            description = description .. "\n\n---\n\n*Related documentation:*\n"
            for _, r in ipairs(results) do
                if r.source_url then
                    description = description .. "- [" .. r.title .. "]("
                        .. r.source_url .. ") (score: "
                        .. string.format("%.2f", r.score) .. ")\n"
                else
                    description = description .. "- " .. (r.title or "untitled")
                        .. " (score: " .. string.format("%.2f", r.score) .. ")\n"
                end
            end
        end
    end

    -- Create the ticket via Jira REST API
    local url = context.config.url
    local email = context.config.email
    local api_token = context.config.api_token

    if not url or not email or not api_token then
        return {
            success = false,
            error = "Missing Jira config: url, email, and api_token are required",
            description_preview = description,
        }
    end

    local auth = base64.encode(email .. ":" .. api_token)

    local payload = json.encode({
        fields = {
            project = { key = params.project },
            summary = params.title,
            description = {
                type = "doc",
                version = 1,
                content = {
                    {
                        type = "paragraph",
                        content = {
                            { type = "text", text = description },
                        },
                    },
                },
            },
            issuetype = { name = "Task" },
            priority = { name = params.priority },
        },
    })

    local resp = http.post(url .. "/rest/api/3/issue", payload, {
        headers = {
            ["Authorization"] = "Basic " .. auth,
            ["Content-Type"] = "application/json",
            ["Accept"] = "application/json",
        },
    })

    if not resp.ok then
        log.error("Jira API error: HTTP " .. resp.status .. " " .. resp.body)
        return {
            success = false,
            error = "Jira API returned " .. resp.status,
            details = resp.body,
        }
    end

    local ticket = resp.json
    local ticket_url = url .. "/browse/" .. ticket.key

    log.info("Created ticket: " .. ticket.key)

    return {
        success = true,
        ticket_key = ticket.key,
        url = ticket_url,
        message = "Created " .. ticket.key .. ": " .. params.title,
    }
end

