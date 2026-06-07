--[[
  Context Harness Agent: doc-coordinator

  Orchestrates the documentation workflow for Context Harness.
  Routes feature work through the five-layer doc hierarchy
  (PRD → ADR → Spec → Design → Runbook), checks which docs
  exist, identifies gaps, and recommends which doc-writer
  agents to invoke next.

  Configuration (add to ctx.toml):

    [agents.script.doc-coordinator]
    path = "agents/doc-coordinator.lua"
    search_limit = "12"

  Usage:
    ctx agent test doc-coordinator --arg feature="drag-and-drop import"
    ctx agent test doc-coordinator --arg action="audit"
]]

agent = {
    name = "doc-coordinator",
    description = "Orchestrates the documentation workflow across the five-layer hierarchy (PRD, ADR, Spec, Design, Runbook)",
    tools = { "search", "get", "sources" },
    arguments = {
        {
            name = "feature",
            description = "Feature or change to coordinate documentation for",
            required = false,
        },
        {
            name = "action",
            description = "What to do: plan (default), audit, or status",
            required = false,
        },
    },
}

function agent.resolve(args, config, context)
    local feature = args.feature or ""
    local action = args.action or "plan"
    local search_limit = tonumber(config.search_limit) or 12

    local system_parts = {
        "You are the Documentation Coordinator for Context Harness.",
        "You manage the five-layer documentation hierarchy:",
        "",
        "| Layer | Dir | Purpose | Authority |",
        "|-------|-----|---------|-----------|",
        "| PRD | docs/prd/ | What to build and why (user perspective) | Product intent |",
        "| ADR | docs/adr/ | Why a specific approach was chosen | Architectural rationale |",
        "| Spec | docs/spec/ | Exactly how the system behaves | Behavioral contract |",
        "| Design | docs/design/ | Exploration, planning, implementation guides | Not authoritative |",
        "| Runbook | docs/runbook/ | Step-by-step operational procedures | Operational reference |",
        "",
        "## Your Responsibilities",
        "",
        "1. **Plan**: Given a feature or change, determine which documents need to be created or updated.",
        "   - Every user-facing feature needs a PRD.",
        "   - Non-trivial technical decisions need an ADR.",
        "   - Behavioral contracts need a spec.",
        "   - Complex implementations need a design doc.",
        "   - Operational tasks need runbooks.",
        "",
        "2. **Audit**: Search the knowledge base for existing docs and identify gaps.",
        "   - Check whether existing PRDs have companion specs and ADRs.",
        "   - Check whether delivered features have runbooks.",
        "   - Flag design docs that should graduate to specs.",
        "",
        "3. **Status**: Report on the current state of documentation for a feature area.",
        "",
        "## Delegation",
        "",
        "You coordinate but do not write docs yourself. Recommend which specialized agent to use:",
        "- **prd-writer** — for Product Requirements Documents",
        "- **adr-writer** — for Architecture Decision Records",
        "- **spec-writer** — for Specifications",
        "- **design-writer** — for Design Documents",
        "- **runbook-writer** — for Runbooks",
        "",
        "## Naming Conventions",
        "",
        "All documents follow the pattern `NNNN-short-kebab-title.md` with zero-padded four-digit numbers.",
        "Each directory has a README.md with an index and a 0000 policy document.",
        "",
        "## Document Flow for New Features",
        "",
        "1. PRD first — captures product intent, goals, success criteria.",
        "2. Design doc (if needed) — explores implementation approaches.",
        "3. ADR(s) — records key technical decisions made during design.",
        "4. Spec — locks down the behavioral contract after implementation.",
        "5. Runbook(s) — covers how to build, deploy, and operate the feature.",
        "",
        "## Output Format",
        "",
        "Present your analysis as:",
        "1. **Current State**: What docs exist for this feature/area.",
        "2. **Gaps**: What's missing from the doc hierarchy.",
        "3. **Action Plan**: Ordered list of docs to create, with the agent to use and the next number in the sequence.",
        "4. **Cross-References**: Which existing docs should link to the new ones.",
    }

    local messages = {}

    if feature ~= "" then
        local queries = {
            feature,
            "PRD " .. feature,
            "spec " .. feature,
            "ADR " .. feature,
            "runbook " .. feature,
            "design " .. feature,
        }

        local all_results = {}
        local seen_ids = {}

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
                "I've searched the knowledge base for existing documentation related to: **" .. feature .. "**",
                "",
            }

            local categorized = {
                prd = {},
                adr = {},
                spec = {},
                design = {},
                runbook = {},
                other = {},
            }

            for _, r in ipairs(all_results) do
                local source = (r.source or ""):lower()
                local title = (r.title or ""):lower()
                if source:find("docs/prd") or title:find("prd") then
                    table.insert(categorized.prd, r)
                elseif source:find("docs/adr") or title:find("adr") then
                    table.insert(categorized.adr, r)
                elseif source:find("docs/spec") or title:find("spec") then
                    table.insert(categorized.spec, r)
                elseif source:find("docs/design") or title:find("design") then
                    table.insert(categorized.design, r)
                elseif source:find("docs/runbook") or title:find("runbook") then
                    table.insert(categorized.runbook, r)
                else
                    table.insert(categorized.other, r)
                end
            end

            local function format_category(label, items)
                if #items == 0 then
                    table.insert(context_parts, "### " .. label .. ": None found")
                    table.insert(context_parts, "")
                    return
                end
                table.insert(context_parts, "### " .. label)
                for i, r in ipairs(items) do
                    table.insert(context_parts, string.format(
                        "%d. **%s** (score: %.2f, source: %s)",
                        i, r.title or "untitled", r.score, r.source or "unknown"
                    ))
                    if r.snippet then
                        table.insert(context_parts, "   > " .. r.snippet:sub(1, 200))
                    end
                end
                table.insert(context_parts, "")
            end

            format_category("PRDs", categorized.prd)
            format_category("ADRs", categorized.adr)
            format_category("Specs", categorized.spec)
            format_category("Design Docs", categorized.design)
            format_category("Runbooks", categorized.runbook)

            if #categorized.other > 0 then
                format_category("Other Related Docs", categorized.other)
            end

            table.insert(context_parts, "Use `get` to retrieve full content for any document, and `search` for additional queries.")

            table.insert(messages, {
                role = "assistant",
                content = table.concat(context_parts, "\n"),
            })
        end
    end

    if action == "audit" then
        table.insert(system_parts, "")
        table.insert(system_parts, "## Current Action: AUDIT")
        table.insert(system_parts, "Perform a comprehensive audit of the documentation. Search for all docs, check for gaps, and report findings.")
    elseif action == "status" then
        table.insert(system_parts, "")
        table.insert(system_parts, "## Current Action: STATUS")
        table.insert(system_parts, "Report the current documentation status for the requested feature area.")
    end

    return {
        system = table.concat(system_parts, "\n"),
        tools = { "search", "get", "sources" },
        messages = messages,
    }
end
