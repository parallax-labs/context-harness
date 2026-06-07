# PRD Policy

This document defines how Context Harness uses **Product Requirements Documents** (PRDs). It applies to all documents in `docs/prd/` and governs what a PRD captures, when to write one, how to structure it, and how it relates to the rest of the documentation hierarchy.

---

## 1. What a PRD is

A **PRD** is the **authoritative statement of product intent**. It defines what we are building, for whom, what problem it solves, and what success looks like — from the user's perspective.

- **Product-level scope:** A PRD describes a feature, capability, or initiative at the level a user or stakeholder would recognize. It does not specify implementation details, data structures, or API shapes — those belong in specs and design docs.
- **Measurable outcomes:** Every PRD includes goals and success criteria that can be evaluated after delivery. Vague aspirations ("improve performance") are not goals; measurable outcomes ("p95 search latency under 200ms for 10k-document workspaces") are.
- **Owned:** Every PRD has an author. The author is responsible for keeping the PRD current through its lifecycle and linking it to the ADRs, specs, and design docs that implement it.

---

## 2. What a PRD is not

- **Not a spec.** A PRD says "users can search across all connectors with a single query." A spec says "The `search` command SHALL accept `--mode keyword|semantic|hybrid` and SHALL return results in the `SearchResultItem` schema." The PRD states the intent; the spec defines the contract.
- **Not a design doc.** A PRD does not explore alternatives or propose architectures. If there are open technical questions, they belong in a design doc. The PRD assumes the problem is worth solving and focuses on what the user needs.
- **Not a task list.** A PRD is not a backlog, sprint plan, or implementation checklist. It informs those artifacts but does not replace them.

---

## 3. When to write a PRD

Write a PRD when:

- **A new user-facing capability is proposed.** Any feature, workflow, or product surface that a user will interact with deserves a PRD before (or shortly after) work begins.
- **A significant change to existing behavior is planned.** If the change affects user expectations, workflows, or compatibility, capture the rationale and requirements in a PRD.
- **Multiple ADRs or specs will be needed.** A PRD serves as the umbrella that ties related technical decisions back to a single product intent.

Do not write a PRD for:

- Internal refactors with no user-visible impact (use an ADR or design doc).
- Bug fixes (use an issue tracker).
- Operational procedures (use a runbook).

---

## 4. PRD structure and required sections

Every PRD MUST include the following metadata and sections:

### Metadata

```
**Status:** Draft | Planned | In Progress | Delivered | Deferred
**Date:** YYYY-MM-DD
**Author:** name or handle
```

### Required sections

| Section | Purpose |
|---------|---------|
| **Problem Statement** | What user or business problem does this solve? Who is affected and how? Be specific. |
| **Target Users** | Personas or use cases this feature serves. |
| **Goals** | Numbered list of measurable outcomes. Each goal is evaluable after delivery. |
| **Non-Goals** | Explicitly out of scope. Prevents scope creep and sets expectations. |
| **User Stories** | Concrete scenarios showing how a user interacts with the feature end-to-end. |
| **Requirements** | High-level functional requirements from the user's perspective. Not implementation details. |
| **Success Criteria** | How we know this shipped correctly — metrics, user outcomes, or acceptance tests. |
| **Dependencies and Risks** | What must land first? What could go wrong? |
| **Related Documents** | Links to ADRs, specs, design docs, and other PRDs. |

### Optional sections

- **Phasing:** If the feature ships incrementally, describe the phases and what each delivers.
- **Competitive Context:** How other tools handle this problem.
- **Open Questions:** Unresolved product decisions. These MUST be resolved before the PRD moves to "In Progress."

---

## 5. Status lifecycle

| Status | Meaning |
|--------|---------|
| **Draft** | Initial write-up. Open questions may exist. Not yet committed to. |
| **Planned** | Product intent is approved. Open questions are resolved. Work has not started. |
| **In Progress** | Implementation is actively underway. |
| **Delivered** | The feature has shipped and success criteria are met (or evaluated). |
| **Deferred** | The feature was planned but deprioritized. The PRD remains for future reference. |

Transitions:

- **Draft → Planned:** All open questions resolved. Goals and success criteria are concrete.
- **Planned → In Progress:** Implementation has begun. Related ADRs and specs should exist or be created.
- **In Progress → Delivered:** Success criteria are met. The PRD links to the specs and ADRs that implemented it.
- **Any → Deferred:** Work is paused indefinitely. Document the reason in the PRD.

A PRD is never deleted. If a feature is abandoned, mark the PRD as Deferred with a rationale.

---

## 6. Relationship to other document types

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

- A PRD **spawns** ADRs and specs. When a technical decision is needed to implement a PRD, write an ADR. When behavior is defined, write a spec.
- A PRD **references** design docs that explore how to implement it.
- When a PRD is delivered, it MUST link to the specs and ADRs that implemented it, creating a traceable chain from product intent to behavioral contract.
- An ADR SHOULD reference the PRD that motivated it.

---

## 7. Summary

| Attribute | PRD |
|-----------|-----|
| **Purpose** | Define what to build and why, from the user's perspective |
| **Authority** | Product intent — not implementation contract |
| **Audience** | Product stakeholders, engineers planning work |
| **When to write** | Before or at the start of a new user-facing capability |
| **Key rule** | Measurable goals and success criteria are mandatory |
| **Lifecycle** | Draft → Planned → In Progress → Delivered (or Deferred) |

PRDs are the starting point of the documentation chain. They answer "what and why" so that ADRs can answer "why this approach," specs can answer "exactly how," and runbooks can answer "how to operate it."
