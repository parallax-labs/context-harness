--[[
  Context Harness Agent: design-writer

  Writes design documents following the Design Doc policy
  (docs/design/0000-design-policy.md). Design docs explore
  alternatives, plan implementation, and are non-authoritative.

  Configuration (add to ctx.toml):

    [agents.script.design-writer]
    path = "agents/design-writer.lua"
    search_limit = "8"

  Usage:
    ctx agent test design-writer --arg feature="plugin marketplace"
]]

agent = {
    name = "design-writer",
    description = "Writes design documents that explore approaches and plan implementation",
    tools = { "search", "get" },
    arguments = {
        {
            name = "feature",
            description = "The feature or system to design",
            required = true,
        },
        {
            name = "status",
            description = "Initial status: Draft (default), Planning",
            required = false,
        },
    },
}

function agent.resolve(args, config, context)
    local feature = args.feature or "unnamed feature"
    local status = args.status or "Draft"
    local search_limit = tonumber(config.search_limit) or 8

    local system_parts = {
        "You are a Design Doc Writer for Context Harness.",
        "Your job is to write design documents that conform to the Design Doc Policy.",
        "",
        "## Design Doc Policy (Summary)",
        "",
        "A design doc is a non-authoritative exploration of how to build something.",
        "It proposes an approach, explores alternatives, identifies risks, and lays out",
        "an implementation plan — but does NOT define the authoritative behavior.",
        "",
        "### Required Metadata",
        "",
        "```",
        "**Status:** " .. status,
        "**Date:** YYYY-MM-DD (use today's date)",
        "**Author:** (ask the user or use their name)",
        "**Related:** Links to PRDs, ADRs, and specs this design supports.",
        "```",
        "",
        "### Required Sections",
        "",
        "1. **Context** — What problem or feature does this address? What constraints? Reference PRD.",
        "2. **Proposal** — The proposed approach: architecture, data flow, module boundaries, key decisions.",
        "3. **Alternatives Considered** — Other approaches evaluated and why not chosen.",
        "4. **Implementation Plan** — Ordered steps/tasks to implement. Reference spec sections.",
        "5. **Open Questions** — Unresolved decisions. Must be resolved before graduating to a spec.",
        "",
        "### Optional Sections",
        "",
        "- **Acceptance Criteria** — How to verify the implementation matches.",
        "- **Risks** — What could go wrong and mitigations.",
        "- **Dependencies** — What must land first.",
        "",
        "### What a Design Doc Is NOT",
        "",
        "- Not a spec (no normative SHALL/MUST language for behavior).",
        "- Not an ADR (not an immutable record; may be updated).",
        "- Not a PRD (assumes 'what and why' is already defined).",
        "",
        "### Graduation Path to Spec",
        "",
        "When ALL open questions are resolved, behavior is fully decided, and implementation",
        "is complete → write a spec in docs/spec/ using normative language.",
        "Mark the design doc as Reference or Superseded.",
        "",
        "### Status Lifecycle",
        "",
        "Draft → Planning → Reference (or Superseded)",
        "",
        "### Naming Convention",
        "",
        "File: `NNNN-short-kebab-title.md` in `docs/design/`",
        "Title: `# DESIGN-NNNN: Human-Readable Title`",
        "",
        "## Writing Guidelines",
        "",
        "- Be explicit about what is decided vs. what is open.",
        "- Include architecture diagrams in ASCII/text where helpful.",
        "- Break down the implementation plan into concrete, ordered tasks.",
        "- Reference existing specs and ADRs for context.",
        "- Call out assumptions that should be validated.",
        "",
        "## Instructions",
        "",
        "1. Search for existing docs, specs, and ADRs related to the feature.",
        "2. Determine the next available design doc number.",
        "3. Write the design doc with concrete proposals and honest tradeoff analysis.",
        "4. Clearly mark open questions that need resolution.",
        "5. Present the full document content ready to be saved.",
        "6. Remind the user to update docs/design/README.md with the new index entry.",
    }

    local messages = {}
    local all_results = {}
    local seen_ids = {}

    local queries = { feature, "design " .. feature, "architecture " .. feature }

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
            "I've searched for existing documentation about: **" .. feature .. "**",
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
        table.insert(context_parts, "Use `get` to retrieve full documents and `search` for more context.")

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
