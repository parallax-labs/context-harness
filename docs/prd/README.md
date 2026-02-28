# Product Requirements Documents

This directory contains Product Requirements Documents (PRDs) for Context Harness.
PRDs capture the **product-level intent** behind features: what we are building,
for whom, and what success looks like. They sit above ADRs and specs in the
documentation hierarchy:

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/`) | Exactly how the system behaves | Behavioral contract |

A PRD may spawn multiple ADRs and specs. An ADR references the PRD that motivated
it. A spec references both for traceability from product intent to implementation.

## Index

| PRD | Title | Status |
|-----|-------|--------|
| [0001](0001-core-context-engine.md) | Core Context Engine | Delivered |
| [0002](0002-browser-search-widget.md) | Browser Search Widget | Delivered |
| [0003](0003-extensibility-platform.md) | Extensibility Platform | Delivered |
| [0004](0004-local-first-embeddings.md) | Local-First Embeddings | Delivered |
| [0005](0005-observability-and-polish.md) | Observability and Polish | Delivered |
| [0006](0006-workspace-refactor-and-library-publishing.md) | Workspace Refactor and Library Publishing | Planned |
| [0007](0007-wasm-client-and-browser-rag.md) | WASM Client and Browser RAG | Planned |
| [0008](0008-tauri-desktop-application.md) | Tauri Desktop Application | Planned |
| [0009](0009-retrieval-quality-and-dogfooding.md) | Retrieval Quality and Dogfooding | Planned |
| [0010](0010-distribution-and-packaging.md) | Distribution and Packaging | Planned |

## Creating a New PRD

1. Copy the template below into a new file named `NNNN-short-title.md` where
   `NNNN` is the next sequential number.
2. Fill in all sections. Be concrete about users, goals, and success criteria.
3. Add an entry to the index table above.
4. Link to related ADRs and specs where they exist.

### Template

```markdown
# PRD-NNNN: Title

**Status:** Draft | Planned | In Progress | Delivered | Deferred
**Date:** YYYY-MM-DD
**Author:** ...

## Problem Statement

What user or business problem does this solve? Who is affected and how?

## Target Users

Specific personas or use cases this feature serves.

## Goals

Numbered list of measurable outcomes this work should achieve.

## Non-Goals

Explicitly out of scope to prevent scope creep.

## User Stories

Concrete scenarios describing how a user interacts with the feature end-to-end.

## Requirements

High-level functional requirements from the user's perspective. Not
implementation details; those belong in specs.

## Success Criteria

How we know this shipped correctly -- metrics, user outcomes, or acceptance
test descriptions.

## Dependencies and Risks

What other work must land first? What could go wrong?

## Related Documents

Links to ADRs, specs, design docs, and other PRDs.
```

## Relationship to Specs and ADRs

PRDs record *what* we are building and *why* from the user's perspective.
ADRs record *why* a specific technical approach was chosen. Specs define
*exactly how* the system behaves as an authoritative contract.

When a PRD is delivered, it should link to the specs and ADRs that implemented
it. When a new PRD is created, it should reference any existing ADRs or specs
that constrain or inform the design.

See [SPEC_POLICY.md](../SPEC_POLICY.md) for the distinction between specs,
design docs, and planning docs.
