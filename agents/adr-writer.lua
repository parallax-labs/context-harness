--[[
  Context Harness Agent: adr-writer

  Writes Architecture Decision Records following the ADR policy
  (docs/adr/0000-adr-policy.md). Pre-loads existing ADRs and related
  docs for consistency and cross-referencing.

  Configuration (add to ctx.toml):

    [agents.script.adr-writer]
    path = "agents/adr-writer.lua"
    search_limit = "8"

  Usage:
    ctx agent test adr-writer --arg decision="use SQLite for vector storage"
]]

agent = {
    name = "adr-writer",
    description = "Writes Architecture Decision Records following the ADR policy",
    tools = { "search", "get" },
    arguments = {
        {
            name = "decision",
            description = "The architectural decision to document",
            required = true,
        },
        {
            name = "status",
            description = "Initial status: Proposed (default), Accepted",
            required = false,
        },
    },
}

function agent.resolve(args, config, context)
    local decision = args.decision or "unnamed decision"
    local status = args.status or "Proposed"
    local search_limit = tonumber(config.search_limit) or 8

    local system_parts = {
        "You are an ADR Writer for Context Harness.",
        "Your job is to write Architecture Decision Records that conform to the ADR Policy.",
        "",
        "## ADR Policy (Summary)",
        "",
        "An ADR is an immutable record of an architectural decision. It captures the context,",
        "the decision itself, alternatives considered, and consequences.",
        "",
        "### Required Metadata",
        "",
        "```",
        "**Status:** " .. status,
        "**Date:** YYYY-MM-DD (use today's date)",
        "```",
        "",
        "### Required Sections",
        "",
        "1. **Context** — Why this decision was needed. What problem, constraint, or opportunity",
        "   prompted it. Include the state of the system at the time.",
        "2. **Decision** — What was decided. Be specific — name the technology, pattern, or approach.",
        "   State it as a declarative fact, not a recommendation.",
        "3. **Alternatives Considered** — What else was evaluated. For each alternative, state what it",
        "   is and why it was rejected. Be honest about tradeoffs.",
        "4. **Consequences** — What follows from this decision. Include both positive outcomes and",
        "   accepted downsides. Note constraints this imposes on future work.",
        "",
        "### Optional Sections",
        "",
        "- **References** — Links to PRDs, specs, benchmarks.",
        "- **Scope** — What part of the system this applies to.",
        "",
        "### Key Rules",
        "",
        "- An accepted ADR is NEVER edited (immutability rule).",
        "- To reverse a decision, write a NEW ADR that supersedes the old one.",
        "- The only permitted edits to accepted ADRs: adding 'Superseded by' status, fixing typos.",
        "- An ADR SHOULD reference the PRD that motivated it.",
        "",
        "### Status Lifecycle",
        "",
        "Proposed → Accepted → (Superseded or Deprecated)",
        "",
        "### Naming Convention",
        "",
        "File: `NNNN-short-kebab-title.md` in `docs/adr/`",
        "Title: `# ADR-NNNN: Human-Readable Title`",
        "",
        "## Instructions",
        "",
        "1. Search for existing ADRs and related docs to understand prior decisions.",
        "2. Determine the next available ADR number.",
        "3. Write the ADR with concrete details — specific technologies, libraries, patterns.",
        "4. Include honest tradeoff analysis in Alternatives and Consequences.",
        "5. Link to the PRD or spec that motivated this decision.",
        "6. Present the full document content ready to be saved.",
        "7. Remind the user to update docs/adr/README.md with the new index entry.",
    }

    local messages = {}
    local all_results = {}
    local seen_ids = {}

    local queries = { decision, "ADR " .. decision, "architecture " .. decision }

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
            "I've searched for existing ADRs and related docs about: **" .. decision .. "**",
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
        table.insert(context_parts, "Review existing ADRs to ensure consistency and avoid contradicting accepted decisions.")

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
