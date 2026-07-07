# Design Documents

This directory contains design documents, implementation plans, and planning
artifacts for Context Harness. Design docs explore alternatives, plan work,
define acceptance criteria, and guide implementation — but they are **not
authoritative** for system behavior. The authoritative contract lives in
specs (`docs/spec/`).

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

See [DESIGN-0000](0000-design-policy.md) for the full policy.

## Index

| Design | Title | Status |
|--------|-------|--------|
| [0000](0000-design-policy.md) | Design Doc Policy | Authoritative |
| [0001](0001-implementation-design.md) | Implementation Design | Reference |
| [0002](0002-sync-progress.md) | Sync and Embed Progress | Planning |
| [0003](0003-file-support-implementation-plan.md) | File Support Implementation Plan | Reference |
| [0004](0004-phase-1-acceptance.md) | Phase 1 Acceptance Criteria | Delivered |
| [0005](0005-deployment-guide.md) | Deployment Guide | Reference |
| [0006](0006-vector-index-acceleration.md) | Vector Index Acceleration | Draft |
| [0007](0007-xdg-config-directories.md) | XDG-Compliant Config and Data Directories | Planning |
| [0008](0008-multi-workspace-mcp-router.md) | Multi-Workspace MCP Router | Draft |
| [0009](0009-workspace-scoped-extensions.md) | Workspace-Scoped Extensions and Request Origin | Draft |

## Creating a New Design Doc

1. Copy the template below into a new file named `NNNN-short-title.md` where
   `NNNN` is the next sequential number.
2. Fill in all sections. Be explicit about what is decided vs. what is open.
3. Add an entry to the index table above.
4. Link to related PRDs, ADRs, and specs where they exist.

### Template

```markdown
# DESIGN-NNNN: Title

**Status:** Draft | Planning | Reference | Superseded
**Date:** YYYY-MM-DD
**Author:** ...
**Related:** Links to PRDs, ADRs, and specs this design supports.

## Context

What problem or feature does this design address? What constraints apply?

## Proposal

The proposed approach. Include architecture, data flow, key decisions.

## Alternatives Considered

What other approaches were evaluated and why they were not chosen.

## Implementation Plan

Ordered steps or tasks to implement this design. Reference spec sections
where applicable.

## Open Questions

Unresolved decisions. These MUST be resolved before this design becomes
an authoritative spec (or the answers are captured in a separate spec).
```

## Relationship to Specs

Design docs support the work but do not replace an authoritative spec.
When implementation of a design is complete, either:

- Write or update an authoritative spec in `docs/spec/` that describes the
  actual behavior, or
- Explicitly state in the design doc that it remains a planning reference and
  that the authoritative contract is the code plus tests.

See [SPEC-0000](../spec/0000-spec-policy.md) for the full policy.
