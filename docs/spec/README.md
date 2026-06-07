# Specifications

This directory contains authoritative specifications for Context Harness.
Specs define **exactly how the system behaves** — they are the contract that
the implementation must satisfy. We program to the spec: the code is correct
when it conforms to the spec.

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

See [SPEC-0000](0000-spec-policy.md) for the full policy.

## Index

| Spec | Title | Status |
|------|-------|--------|
| [0000](0000-spec-policy.md) | Spec Policy | Authoritative |
| [0002](0002-workspace-refactor.md) | Workspace Refactor | Authoritative |
| [0003](0003-hybrid-scoring.md) | Hybrid Scoring | Authoritative |
| [0004](0004-file-support.md) | Multi-Format File Support | Authoritative |
| [0005](0005-usage-contract.md) | Usage Contract | Authoritative |
| [0006](0006-json-schemas.md) | JSON Schemas | Authoritative |
| [0007](0007-extension-registries.md) | Extension Registries | Authoritative |
| [0008](0008-lua-connectors.md) | Lua Scripted Connectors | Authoritative |
| [0009](0009-lua-tools.md) | Lua MCP Tool Extensions | Authoritative |
| [0010](0010-rust-extension-traits.md) | Rust Extension Traits | Authoritative |
| [0011](0011-mcp-agents.md) | MCP Agents | Authoritative |
| [0013](0013-direct-llm-inference-chat.md) | Direct LLM Inference Chat | Draft |

## Creating a New Spec

1. Copy the template below into a new file named `NNNN-short-title.md` where
   `NNNN` is the next sequential number.
2. Fill in all sections. Use normative language (SHALL, MUST, MAY).
3. Add an entry to the index table above.
4. Commit alongside the implementation, or retroactively once behavior is decided.

### Template

```markdown
# SPEC-NNNN: Title

**Status:** Draft | Authoritative | Superseded by [SPEC-NNNN](NNNN-title.md) | Deprecated
**Date:** YYYY-MM-DD
**Scope:** What system area this spec covers.

## Overview

Brief summary of what this spec defines.

## Definitions

Key terms used throughout the spec.

## Requirements

Definitive behavioral statements. Use SHALL / MUST for required behavior,
MAY for permitted but optional behavior. No options or recommendations —
only one defined behavior per requirement.

## Acceptance Criteria

How to verify the implementation conforms to this spec.
```

## Relationship to Other Docs

Specs are the **authoritative source of truth** for system behavior. See
[SPEC-0000](0000-spec-policy.md) for the full policy on how specs differ from
design docs and planning docs.

- A **PRD** defines *what* to build and *why*. A spec defines *how* it behaves.
- An **ADR** records *why* a specific technical approach was chosen.
- A **Design doc** explores alternatives or plans work. It is not authoritative.
- A **Runbook** describes how to operate the system. It references specs but does not define behavior.

When a spec changes, related ADRs should be reviewed. When a PRD is delivered,
its specs should be linked from the PRD.
