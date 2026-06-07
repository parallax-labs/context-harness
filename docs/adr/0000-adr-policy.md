# ADR Policy

This document defines how Context Harness uses **Architecture Decision Records** (ADRs). It applies to all documents in `docs/adr/` and governs what an ADR captures, when to write one, how to structure it, and how it relates to the rest of the documentation hierarchy.

---

## 1. What an ADR is

An **ADR** is an **immutable record of an architectural decision**. It captures the context that led to the decision, the decision itself, the alternatives that were considered, and the consequences — both positive and negative.

- **Point-in-time record:** An ADR documents the decision as it was made, with the information available at the time. It is a historical artifact.
- **Architectural scope:** ADRs cover decisions that affect the system's structure, key technology choices, integration patterns, data models, or cross-cutting concerns. They do not cover product requirements (PRDs) or behavioral contracts (specs).
- **Concrete:** An ADR names the specific technology, pattern, or approach that was chosen. "We chose SQLite" is an ADR. "We should use a database" is not.

---

## 2. What an ADR is not

- **Not a spec.** An ADR records *why* a decision was made. A spec defines *how* the system behaves. An ADR that says "we chose paragraph-boundary chunking" does not define the chunking algorithm — that belongs in a spec.
- **Not a design doc.** An ADR does not explore open questions or propose multiple approaches for future evaluation. By the time an ADR is written, the decision has been made. Open exploration belongs in a design doc.
- **Not a PRD.** An ADR does not define what to build or why from the user's perspective. It records a technical decision made in service of a PRD.
- **Not editable after acceptance.** Once an ADR is accepted, its content is frozen. If the decision is reversed, write a new ADR that supersedes it.

---

## 3. When to write an ADR

Write an ADR when:

- **A non-trivial architectural choice is made.** If the decision affects multiple modules, introduces a new dependency, changes the data model, or constrains future options, it deserves an ADR.
- **Alternatives were seriously considered.** If the team evaluated multiple approaches before choosing one, the rationale should be preserved.
- **The decision would surprise a new contributor.** If someone joining the project would ask "why did you do it this way?", the answer belongs in an ADR.

Do not write an ADR for:

- Trivial implementation choices (variable naming, minor refactors).
- Product-level decisions (use a PRD).
- Behavioral contracts (use a spec).
- Operational procedures (use a runbook).

---

## 4. ADR structure and required sections

Every ADR MUST include the following metadata and sections:

### Metadata

```
**Status:** Proposed | Accepted | Superseded by [ADR-NNNN](NNNN-title.md) | Deprecated
**Date:** YYYY-MM-DD
```

### Required sections

| Section | Purpose |
|---------|---------|
| **Context** | Why this decision was needed. What problem, constraint, or opportunity prompted it. Include relevant technical context and the state of the system at the time. |
| **Decision** | What was decided. Be specific — name the technology, pattern, library, or approach. State it as a declarative fact, not a recommendation. |
| **Alternatives Considered** | What else was evaluated. For each alternative, state what it is and why it was rejected. Be honest about tradeoffs. |
| **Consequences** | What follows from this decision. Include both positive outcomes and accepted downsides. Note any constraints this decision imposes on future work. |

### Optional sections

- **References:** Links to PRDs, specs, external documentation, or benchmarks that informed the decision.
- **Scope:** What part of the system this decision applies to, if not obvious from the title.

---

## 5. Status lifecycle and immutability

| Status | Meaning |
|--------|---------|
| **Proposed** | The decision is drafted but not yet accepted. Open for discussion. |
| **Accepted** | The decision is final and implementation should conform to it. |
| **Superseded** | A newer ADR has replaced this decision. The superseding ADR is linked. |
| **Deprecated** | The decision is no longer relevant (e.g., the feature was removed). |

### Immutability rule

**An accepted ADR is never edited.** The content is frozen as a historical record. If the decision needs to change:

1. Write a **new ADR** that describes the new context and new decision.
2. Mark the old ADR as **Superseded by [ADR-NNNN](NNNN-title.md)**.
3. The new ADR SHOULD reference the old one and explain what changed.

The only permitted edits to an accepted ADR are:

- Adding a "Superseded by" status line.
- Fixing broken links or typos that do not change the meaning.

This immutability ensures the historical record is trustworthy. Future contributors can trace the evolution of decisions by following the supersession chain.

---

## 6. Numbering and naming

- ADRs are numbered sequentially: `0001`, `0002`, etc.
- File names use the pattern `NNNN-short-kebab-title.md`.
- The title in the file uses the pattern `# ADR-NNNN: Human-Readable Title`.
- Numbers are never reused, even if an ADR is deprecated or superseded.
- This policy document is `0000-adr-policy.md` and is exempt from the immutability rule (it may be updated as the policy evolves).

---

## 7. Relationship to other document types

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

- An ADR SHOULD reference the **PRD** that motivated the decision.
- An ADR MAY reference **specs** that define the behavior resulting from the decision.
- When a **spec** changes in a way that reverses an ADR, the ADR MUST be marked Superseded and a new ADR written.
- A **design doc** may precede an ADR — the design explores options, and the ADR records which option was chosen.

---

## 8. Summary

| Attribute | ADR |
|-----------|-----|
| **Purpose** | Record why a specific architectural approach was chosen |
| **Authority** | Historical rationale — not a behavioral contract |
| **Audience** | Engineers, architects, future contributors |
| **When to write** | When a non-trivial technical decision is made |
| **Key rule** | Accepted ADRs are immutable; supersede, don't edit |
| **Lifecycle** | Proposed → Accepted → (Superseded or Deprecated) |

ADRs are the project's architectural memory. They answer "why did we do it this way?" so that future decisions are informed by past reasoning rather than repeated from scratch.
