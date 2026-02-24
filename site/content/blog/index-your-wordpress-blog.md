+++
title = "Recipe: Index Your WordPress Blog"
description = "Write a 60-line Lua connector that parses a WordPress XML export and turns every post into a searchable document."
date = 2026-02-23

[taxonomies]
tags = ["recipe", "lua"]
+++

WordPress lets you export your entire blog as an XML file (WXR format). With a short Lua connector, you can parse that export and make every post searchable by your AI tools — no plugins, no WordPress API, no PHP.

This is what you need when you want to ask Cursor *"What did I write about Kubernetes last year?"* and get an actual answer grounded in your own writing.

### Step 1: Export your WordPress content

In your WordPress admin, go to **Tools → Export → All content → Download Export File**. You'll get an XML file like `wordpress-export.xml`.

### Step 2: Write the connector

Create `connectors/wordpress.lua` and put the XML file next to it:

```
connectors/
  wordpress.lua
  wordpress-export.xml
```

Here's the full connector — it pattern-matches the WXR XML to extract posts:

```lua
connector = {}
connector.name = "wordpress"
connector.description = "Parse WordPress WXR XML export into individual posts"

function connector.scan(config)
    local xml = fs.read(config.file or "wordpress-export.xml")
    local items = {}

    for item_block in xml:gmatch("<item>(.-)</item>") do
        local title = item_block:match("<title>(.-)</title>") or "Untitled"
        local post_type = item_block:match("<wp:post_type><!%[CDATA%[(.-)%]%]>") or "post"

        -- Only index posts and pages, skip attachments/nav items
        if post_type == "post" or post_type == "page" then
            local link = item_block:match("<link>(.-)</link>") or ""
            local pub_date = item_block:match("<pubDate>(.-)</pubDate>") or ""
            local creator = item_block:match("<dc:creator><!%[CDATA%[(.-)%]%]>") or ""

            -- Extract body from CDATA
            local body = item_block:match(
                "<content:encoded><!%[CDATA%[(.-)%]%]></content:encoded>"
            ) or ""

            -- Strip HTML tags for cleaner chunks
            body = body:gsub("<[^>]+>", " ")
            body = body:gsub("&nbsp;", " ")
            body = body:gsub("&amp;", "&")
            body = body:gsub("&lt;", "<")
            body = body:gsub("&gt;", ">")
            body = body:gsub("%s+", " ")
            body = body:match("^%s*(.-)%s*$") or body

            -- Extract categories and tags
            local tags = {}
            for tag in item_block:gmatch('<category domain="post_tag".-<!%[CDATA%[(.-)%]%]>') do
                table.insert(tags, tag)
            end
            for cat in item_block:gmatch('<category domain="category".-<!%[CDATA%[(.-)%]%]>') do
                table.insert(tags, cat)
            end

            if #body > 50 then
                table.insert(items, {
                    id = link ~= "" and link or title,
                    title = title,
                    body = body,
                    url = link,
                    metadata = {
                        author = creator,
                        published = pub_date,
                        type = post_type,
                        tags = table.concat(tags, ", "),
                    },
                })
            end
        end
    end

    log.info(string.format("Parsed %d posts/pages from WordPress export", #items))
    return items
end

return connector
```

### Step 3: Configure and sync

Add to your `ctx.toml`:

```toml
[connectors.script.wordpress]
path = "connectors/wordpress.lua"
file = "wordpress-export.xml"
```

Then sync:

```bash
$ ctx sync script:wordpress
sync script:wordpress
  fetched: 147 items
  upserted documents: 147
  chunks written: 892
ok
```

Every blog post is now chunked, indexed, and searchable.

### Step 4: Search your writing

```bash
$ ctx search "kubernetes deployment"
1. [0.91] script:wordpress / How I Migrated to K8s
   "After three weekends of YAML wrangling, I finally moved everything..."
2. [0.78] script:wordpress / DevOps Lessons from 2024
   "The biggest win was containerizing the legacy monolith..."
```

### Step 5: Connect to Cursor

Start the server and add it to your workspace:

```bash
$ ctx serve mcp
```

`.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "my-blog": {
      "url": "http://127.0.0.1:7331/mcp"
    }
  }
}
```

Now ask Cursor: *"Search my blog for posts about deployment automation"* — and it pulls from your actual writing.

### Tips

- **Comments too?** Add another `gmatch` loop for `<wp:comment>` blocks inside each item. Append them to the post body or emit them as separate documents.
- **Multiple exports?** Use `fs.list(".")` with a glob and loop over all XML files — each gets parsed separately but lands in the same database.
- **Incremental updates?** Re-export from WordPress periodically and re-run `ctx sync`. The connector upserts by `id` (the post URL), so unchanged posts are skipped.
- **HTML rendering?** The connector strips HTML tags for cleaner text. If you want to preserve formatting (code blocks, lists), replace the `gsub("<[^>]+>", " ")` line with more selective stripping.
