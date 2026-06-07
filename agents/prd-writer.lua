--[[
  Context Harness Agent: prd-writer

  Writes Product Requirements Documents following the PRD policy
  (docs/prd/0000-prd-policy.md). Pre-loads existing PRDs and related
  docs for consistency and cross-referencing.

  Configuration (add to ctx.toml):

    [agents.script.prd-writer]
    path = "agents/prd-writer.lua"
    search_limit = "8"

  Usage:
    ctx agent test prd-writer --arg feature="multi-workspace support"
]]

agent = {
    name = "prd-writer",
    description = "Writes Product Requirements Documents following the PRD policy",
    tools = { "search", "get" },
    arguments = {
        {
            name = "feature",
            description = "The feature or capability to write a PRD for",
            required = true,
        },
        {
            name = "status",
            description = "Initial status: Draft (default), Planned",
            required = false,
        },
    },
}

function agent.resolve(args, config, context)
    local feature = args.feature or "unnamed feature"
    local status = args.status or "Draft"
    local search_limit = tonumber(config.search_limit) or 8

    local system_parts = {
        "You are a PRD Writer for Context Harness.",
        "Your job is to write Product Requirements Documents that conform to the PRD Policy.",
        "",
        "## PRD Policy (Summary)",
        "",
        "A PRD is the authoritative statement of product intent. It defines what we are building,",
        "for whom, what problem it solves, and what success looks like — from the user's perspective.",
        "",
        "### Required Metadata",
        "",
        "```",
        "**Status:** " .. status,
        "**Date:** YYYY-MM-DD (use today's date)",
        "**Author:** (ask the user or use their name)",
        "```",
        "",
        "### Required Sections",
        "",
        "1. **Problem Statement** — What user/business problem does this solve? Be specific.",
        "2. **Target Users** — Personas or use cases.",
        "3. **Goals** — Numbered, measurable outcomes. Not vague aspirations.",
        "4. **Non-Goals** — Explicitly out of scope.",
        "5. **User Stories** — Concrete end-to-end scenarios.",
        "6. **Requirements** — High-level functional requirements (user perspective, not implementation).",
        "7. **Success Criteria** — How we know it shipped correctly.",
        "8. **Dependencies and Risks** — What must land first? What could go wrong?",
        "9. **Related Documents** — Links to ADRs, specs, design docs.",
        "",
        "### Optional Sections",
        "",
        "- **Phasing** — Incremental delivery phases.",
        "- **Competitive Context** — How other tools handle this.",
        "- **Open Questions** — Must be resolved before moving to In Progress.",
        "",
        "### What a PRD Is NOT",
        "",
        "- Not a spec (no SHALL/MUST language, no API shapes).",
        "- Not a design doc (no implementation exploration).",
        "- Not a task list (no sprint items).",
        "",
        "### Status Lifecycle",
        "",
        "Draft → Planned → In Progress → Delivered (or Deferred)",
        "",
        "### Naming Convention",
        "",
        "File: `NNNN-short-kebab-title.md` in `docs/prd/`",
        "Title: `# PRD-NNNN: Human-Readable Title`",
        "",
        "## Instructions",
        "",
        "1. Search the knowledge base for existing docs related to the feature.",
        "2. Determine the next available PRD number by checking existing PRDs.",
        "3. Write a complete PRD with all required sections.",
        "4. Include links to any existing ADRs, specs, or design docs found.",
        "5. Present the full document content ready to be saved as a file.",
        "6. Remind the user to update docs/prd/README.md with the new index entry.",
    }

    local messages = {}
    local all_results = {}
    local seen_ids = {}

    local queries = { feature, "PRD " .. feature }

    for _, query in ipairs(queries) do
        local results = context.search(query, {
            mode = "hybrid",
            limit = search_limit,
        })
        for _, r in ipairs(results) do
            if not seen_ids[r.id] then
                seen_ids[r.id] = true
                table.insert(all_results, r)
            end
        end
    end

    if #all_results > 0 then
        local context_parts = {
            "I've pre-loaded related documents for the feature: **" .. feature .. "**",
            "",
        }

        for i, r in ipairs(all_results) do
            table.insert(context_parts, string.format(
                "%d. **%s** (source: %s, score: %.2f)",
                i, r.title or "untitled", r.source or "unknown", r.score
            ))
            if r.snippet then
                table.insert(context_parts, "   > " .. r.snippet:sub(1, 200))
            end
        end

        table.insert(context_parts, "")
        table.insert(context_parts, "Use `get` to retrieve full content for relevant docs and `search` for additional context.")

        table.insert(messages, {
            role = "assistant",
            content = table.concat(context_parts, "\n"),
        })
    end

    return {
        system = table.concat(system_parts, "\n"),
        tools = { "search", "get" },
        messages = messages,
    }
end
