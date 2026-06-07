--[[
  Context Harness Agent: spec-writer

  Writes authoritative specifications following the Spec policy
  (docs/spec/0000-spec-policy.md). Specs use normative language
  (SHALL, MUST, MAY) and define the behavioral contract that
  implementation must satisfy.

  Configuration (add to ctx.toml):

    [agents.script.spec-writer]
    path = "agents/spec-writer.lua"
    search_limit = "10"

  Usage:
    ctx agent test spec-writer --arg feature="hybrid search API"
]]

agent = {
    name = "spec-writer",
    description = "Writes authoritative specifications using normative language (SHALL, MUST, MAY)",
    tools = { "search", "get" },
    arguments = {
        {
            name = "feature",
            description = "The feature or behavior to specify",
            required = true,
        },
        {
            name = "status",
            description = "Initial status: Draft (default), Authoritative",
            required = false,
        },
    },
}

function agent.resolve(args, config, context)
    local feature = args.feature or "unnamed feature"
    local status = args.status or "Draft"
    local search_limit = tonumber(config.search_limit) or 10

    local system_parts = {
        "You are a Spec Writer for Context Harness.",
        "Your job is to write authoritative specifications that conform to the Spec Policy.",
        "",
        "## Spec Policy (Summary)",
        "",
        "A spec is the authoritative description of behavior. It is the contract that the",
        "implementation must satisfy. We program to the spec: the code is correct when it",
        "conforms to the spec.",
        "",
        "### Required Metadata",
        "",
        "```",
        "**Status:** " .. status,
        "**Date:** YYYY-MM-DD (use today's date)",
        "**Scope:** What system area this spec covers.",
        "```",
        "",
        "### Required Sections",
        "",
        "1. **Overview** — Brief summary of what this spec defines.",
        "2. **Definitions** — Key terms used throughout.",
        "3. **Requirements** — Definitive behavioral statements using normative language:",
        "   - SHALL / MUST = required behavior",
        "   - MAY = permitted but optional",
        "   - No 'could', 'might', 'we recommend', 'implementation may choose'",
        "   - One defined behavior per requirement — no options.",
        "4. **Acceptance Criteria** — How to verify implementation conforms.",
        "",
        "### What a Spec Is NOT",
        "",
        "- Not a design doc (no exploration of alternatives, no open questions).",
        "- Not a planning doc (no 'implementation should/will/could').",
        "- Not a recommendation (no 'we suggest').",
        "",
        "### When to Write a Spec",
        "",
        "PREFERRED: Implement the feature first, then write the spec describing actual behavior.",
        "ALTERNATIVE: Write the spec first only when behavior is fully decided.",
        "",
        "### Status Lifecycle",
        "",
        "Draft → Authoritative → (Superseded or Deprecated)",
        "",
        "### Naming Convention",
        "",
        "File: `NNNN-short-kebab-title.md` in `docs/spec/`",
        "Title: `# SPEC-NNNN: Human-Readable Title`",
        "",
        "## Writing Guidelines",
        "",
        "- Use active voice: 'The server SHALL return...' not 'It is returned by...'",
        "- Be concrete: name config keys, CLI flags, HTTP paths, data formats.",
        "- Include examples with actual input/output pairs.",
        "- Cross-reference related specs, ADRs, and PRDs.",
        "- Number requirements for easy reference (e.g., R1, R2, R3).",
        "",
        "## Instructions",
        "",
        "1. Search for existing specs, ADRs, and the source code to understand current behavior.",
        "2. Determine the next available spec number.",
        "3. Write the spec with normative language and concrete details.",
        "4. Include acceptance criteria that can be turned into tests.",
        "5. Reference the PRD and any ADRs that informed the behavior.",
        "6. Present the full document content ready to be saved.",
        "7. Remind the user to update docs/spec/README.md with the new index entry.",
    }

    local messages = {}
    local all_results = {}
    local seen_ids = {}

    local queries = { feature, "spec " .. feature, feature .. " behavior" }

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

        local specs = {}
        local other = {}

        for _, r in ipairs(all_results) do
            local source = (r.source or ""):lower()
            if source:find("docs/spec") then
                table.insert(specs, r)
            else
                table.insert(other, r)
            end
        end

        if #specs > 0 then
            table.insert(context_parts, "### Existing Specs")
            for i, r in ipairs(specs) do
                table.insert(context_parts, string.format(
                    "%d. **%s** (source: %s, score: %.2f)",
                    i, r.title or "untitled", r.source or "unknown", r.score
                ))
            end
            table.insert(context_parts, "")
        end

        if #other > 0 then
            table.insert(context_parts, "### Related Docs")
            for i, r in ipairs(other) do
                table.insert(context_parts, string.format(
                    "%d. **%s** (source: %s, score: %.2f)",
                    i, r.title or "untitled", r.source or "unknown", r.score
                ))
                if r.snippet then
                    table.insert(context_parts, "   > " .. r.snippet:sub(1, 200))
                end
            end
            table.insert(context_parts, "")
        end

        table.insert(context_parts, "Use `get` to read existing specs in full before writing to ensure consistency.")

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
