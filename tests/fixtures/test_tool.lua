--[[
  Test fixture tool for integration tests.
  Returns predictable output for automated verification.
]]

tool = {
    name = "test_tool",
    version = "0.1.0",
    description = "Test tool for automated testing",
    parameters = {
        {
            name = "input",
            type = "string",
            required = true,
            description = "Input value",
        },
        {
            name = "uppercase",
            type = "boolean",
            required = false,
            description = "Convert input to uppercase",
            default = false,
        },
    },
}

function tool.execute(params, context)
    local output = params.input

    if params.uppercase then
        output = string.upper(output)
    end

    -- Verify context bridge is available
    local sources = context.sources()

    return {
        output = output,
        input_length = #params.input,
        source_count = #sources,
        config_keys = 0,
    }
end

