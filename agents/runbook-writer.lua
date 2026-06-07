--[[
  Context Harness Agent: runbook-writer

  Writes operational runbooks following the Runbook policy
  (docs/runbook/0000-runbook-policy.md). Runbooks are step-by-step
  procedures for discrete operational tasks.

  Configuration (add to ctx.toml):

    [agents.script.runbook-writer]
    path = "agents/runbook-writer.lua"
    search_limit = "8"

  Usage:
    ctx agent test runbook-writer --arg task="deploy to kubernetes"
]]

agent = {
    name = "runbook-writer",
    description = "Writes operational runbooks with step-by-step procedures",
    tools = { "search", "get" },
    arguments = {
        {
            name = "task",
            description = "The operational task this runbook covers",
            required = true,
        },
        {
            name = "status",
            description = "Initial status: Draft (default), Active",
            required = false,
        },
    },
}

function agent.resolve(args, config, context)
    local task = args.task or "unnamed task"
    local status = args.status or "Draft"
    local search_limit = tonumber(config.search_limit) or 8

    local system_parts = {
        "You are a Runbook Writer for Context Harness.",
        "Your job is to write operational runbooks that conform to the Runbook Policy.",
        "",
        "## Runbook Policy (Summary)",
        "",
        "A runbook is a step-by-step operational procedure for a discrete task.",
        "It tells an operator exactly what to do, in what order, and how to verify",
        "success — without requiring deep knowledge of system internals.",
        "",
        "### Required Metadata",
        "",
        "```",
        "**Status:** " .. status,
        "**Date:** YYYY-MM-DD (use today's date)",
        "**Author:** (ask the user or use their name)",
        "**Last Verified:** YYYY-MM-DD (use today's date for new runbooks)",
        "```",
        "",
        "### Required Sections",
        "",
        "1. **Purpose** — 1-2 sentences: what this accomplishes and when to use it.",
        "2. **Prerequisites** — Tools, env vars, access, prior runbooks needed.",
        "3. **Steps** — Numbered, sequential. Each step:",
        "   - One action per step.",
        "   - Show the exact command in a fenced code block.",
        "   - Show the expected output.",
        "   - Never assume context ('run the build' → `cargo build --release`).",
        "4. **Verification** — Concrete checks to confirm success.",
        "5. **Troubleshooting** — Table of common failure modes and fixes.",
        "",
        "### Optional Sections",
        "",
        "- **Rollback** — How to undo (required for destructive operations).",
        "- **Notes** — Additional context or gotchas.",
        "- **Related Runbooks** — Commonly performed before/after.",
        "",
        "### Writing Effective Steps",
        "",
        "- Number every step.",
        "- One action per step.",
        "- Show the command in a code block with exact flags.",
        "- Show expected output after each command.",
        "- Use conditional steps sparingly.",
        "- Never assume context — spell out every command.",
        "",
        "### What a Runbook Is NOT",
        "",
        "- Not a design doc (no architecture exploration).",
        "- Not a spec (no normative behavioral contract).",
        "- Not a tutorial (optimized for execution, not learning).",
        "",
        "### Review Cadence",
        "",
        "- Re-verify quarterly.",
        "- Update Last Verified date after each successful execution.",
        "- Update immediately when found to be inaccurate.",
        "",
        "### Status Lifecycle",
        "",
        "Draft → Active → Deprecated",
        "",
        "### Naming Convention",
        "",
        "File: `NNNN-short-kebab-title.md` in `docs/runbook/`",
        "Title: `# RUNBOOK-NNNN: Human-Readable Title`",
        "",
        "## Instructions",
        "",
        "1. Search for existing runbooks and related specs/docs.",
        "2. Determine the next available runbook number.",
        "3. Write the runbook with concrete, copy-pasteable commands.",
        "4. Include real expected outputs, not placeholders.",
        "5. Add a troubleshooting table with at least 3 common issues.",
        "6. Reference related specs and runbooks.",
        "7. Present the full document content ready to be saved.",
        "8. Remind the user to update docs/runbook/README.md with the new index entry.",
    }

    local messages = {}
    local all_results = {}
    local seen_ids = {}

    local queries = { task, "runbook " .. task, task .. " procedure" }

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
            "I've searched for existing documentation about: **" .. task .. "**",
            "",
        }

        local runbooks = {}
        local other = {}

        for _, r in ipairs(all_results) do
            local source = (r.source or ""):lower()
            if source:find("docs/runbook") then
                table.insert(runbooks, r)
            else
                table.insert(other, r)
            end
        end

        if #runbooks > 0 then
            table.insert(context_parts, "### Existing Runbooks")
            for i, r in ipairs(runbooks) do
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

        table.insert(context_parts, "Use `get` to read existing runbooks for style consistency.")

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
