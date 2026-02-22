-- Test connector that returns hardcoded items.
-- Used by integration tests to verify the Lua connector runtime.

connector = {
    name = "test",
    version = "0.1.0",
    description = "Test connector returning hardcoded items",
}

function connector.scan(config)
    local prefix = config.prefix or "test"
    local count = tonumber(config.count) or 3

    log.info("Test connector scanning with prefix=" .. prefix .. " count=" .. count)

    local items = {}
    for i = 1, count do
        table.insert(items, {
            source_id = prefix .. "-" .. i,
            title = "Test Item " .. i,
            body = "This is the body content for test item number " .. i .. ". It has enough text to be meaningful for chunking and search purposes.",
            author = "test-author",
            source_url = "https://example.com/items/" .. i,
            content_type = "text/plain",
            updated_at = "2025-01-15T10:00:00Z",
            created_at = "2025-01-01T00:00:00Z",
        })
    end

    return items
end

