+++
title = "Give AI your project context. Put it to work."
description = "Context Harness turns local project knowledge into reusable profiles for the AI clients you already use and bounded agents you can run, inspect, and recover."
sort_by = "weight"
template = "docs/section.html"

[extra]
+++

Context Harness gives AI systems a reliable bridge to the knowledge and tools
that already live in your project. Ingest documentation, code, and operational
knowledge from multiple sources, keep the canonical index in local SQLite, and
choose how AI should use it:

- **Profiles** make Cursor, Claude, and custom clients start with a consistent
  role, relevant tools, and optional project-aware context.
- **Agents** let Context Harness run bounded work itself with scoped tools,
  permissions, approvals, durable history, inspection, and recovery.

| Profiles | Agents |
|---|---|
| Context Harness prepares the role and project context; your AI client owns the conversation. | Context Harness owns a bounded model-and-tool run with policy and durable history. |
| Best when your team already works in Cursor, Claude, or another client. | Best when the task needs controlled execution, inspection, approval, or recovery. |

Already using an AI client and want better, repeatable project context? Start
with **Profiles**. Want a task runner that Context Harness can govern and leave
an audit trail for? Start with **Agents**. Both build on the same local-first
context layer, so you can adopt one without committing to the other.
