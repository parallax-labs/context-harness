--[[
  Context Harness Tool: echo

  A minimal tool that echoes parameters back, optionally searching the
  knowledge base. Useful for testing the tool runtime, parameter validation,
  and the context bridge.

  Configuration:
    [tools.script.echo]
    path = "examples/tools/echo.lua"
    greeting = "Hello"

  Test:
    ctx tool test examples/tools/echo.lua --param message="Hello world"
]]

tool = {
    name = "echo",
    version = "1.0.0",
    description = "Echo parameters back with optional knowledge base search",
    parameters = {
        {
            name = "message",
            type = "string",
            required = true,
            description = "Message to echo",
        },
        {
            name = "search",
            type = "boolean",
            required = false,
            description = "If true, search the knowledge base for the message",
            default = false,
        },
        {
            name = "limit",
            type = "integer",
            required = false,
            description = "Max search results",
            default = 3,
        },
    },
}

function tool.execute(params, context)
    local greeting = context.config.greeting or "Echo"
    local result = {
        echo = greeting .. ": " .. params.message,
        params = params,
    }

    -- Optionally search the knowledge base
    if params.search then
        local search_results = context.search(params.message, {
            mode = "keyword",
            limit = params.limit or 3,
        })

        result.search_results = {}
        for i, r in ipairs(search_results) do
            result.search_results[i] = {
                title = r.title,
                score = r.score,
                snippet = r.snippet,
            }
        end
        result.result_count = #search_results
    end

    -- Show available sources
    local sources = context.sources()
    result.source_count = #sources

    log.info("Echoed: " .. params.message)
    return result
end

