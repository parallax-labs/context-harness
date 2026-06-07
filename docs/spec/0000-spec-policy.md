# Spec Policy

This document defines how Context Harness uses **specs** and how they differ from **design** or **planning** documents. It applies to all docs in `docs/` that describe behavior or contracts.

---

## 1. What a spec is

A **spec** is the **authoritative description of behavior**. It is the contract that the implementation must satisfy. We program to the spec: the code is correct when it conforms to the spec; the spec is not updated to match the code after the fact.

- **Single source of truth:** The spec states what the system does. There are no "options" or "recommendations" — only one defined behavior per requirement.
- **Normative language:** Use SHALL / MUST for required behavior, MAY for permitted but optional behavior. Avoid "could", "might", "we recommend", "implementation may choose".
- **Testable:** Acceptance criteria and tests verify that the implementation matches the spec. When in doubt, the spec wins.

---

## 2. What a spec is not

- **Design doc:** A design doc explores alternatives, recommends an approach, or leaves decisions to the implementation. It is useful for planning and for handing off work, but it is not the authority for behavior.
- **Planning / contract-for-work:** A document that says "the implementer should do X; implementation will decide Y" is a contract for the work, not a spec for the feature. It tells the implementer what to build, but it does not define the single authoritative behavior.

Design and planning docs are valuable. They belong in `docs/` and should be clearly labeled (e.g. "Design", "Planning", "Contract for implementation") so they are not mistaken for specs.

---

## 3. When to write a spec

**Preferred:** Implement the feature (or a minimal slice), then write the spec that describes the **actual behavior**. The spec documents what the system does. That keeps the spec authoritative and option-free: no guessing, no "TBD by implementation".

**Alternative:** Write the spec first only when the behavior is already fully decided (e.g. external standard, or a prior design that has been locked). In that case the spec uses normative language and leaves no open choices; implementation then conforms to it.

Do not publish a doc as a "spec" if it still contains options, recommendations, or "implementation will decide". Either resolve those into a single decision and make the doc authoritative, or label it as a design/planning doc and write the real spec after implementation.

---

## 4. Spec structure and language

- **Title and status:** Include "Spec" in the title (e.g. "Hybrid Scoring Spec") and a status line (e.g. "Status: Authoritative" or "Status: Draft — not yet implemented").
- **Requirements:** Write requirements as definitive statements. Prefer "The system SHALL ...", "Sync SHALL emit progress on stderr in the following format: ...", "PDF extraction MUST use ...".
- **No options:** If multiple approaches were considered, the spec states the chosen one only. Move alternatives to a separate design doc or drop them.
- **Concrete details:** Specify formats, config keys, error codes, and CLI output shape. "TBD" is acceptable only for not-yet-implemented sections; replace with concrete behavior before or when the feature ships.

---

## 5. Relationship to implementation and tests

- **Implementation** exists to satisfy the spec. Changes to behavior require updating the spec first (or in the same change); the spec is not retrofitted to match code without going through this policy.
- **Tests** (unit, integration, or acceptance) should be written to assert spec compliance. The spec is the reference for what "correct" means.
- **Existing authoritative specs** (e.g. `spec/0003-hybrid-scoring.md`) remain the contract. New features that touch the same area must conform to them or the spec must be updated with a clear rationale.

---

## 6. Where docs live

All documentation lives in subdirectories of `docs/`, each with its own policy and numbering:

| Directory | Policy | Purpose |
|-----------|--------|---------|
| `docs/prd/` | [PRD-0000](../prd/0000-prd-policy.md) | Product requirements — what to build and why |
| `docs/adr/` | [ADR-0000](../adr/0000-adr-policy.md) | Architecture decisions — why a specific approach was chosen |
| `docs/spec/` | This document | Specifications — exactly how the system behaves |
| `docs/design/` | [DESIGN-0000](../design/0000-design-policy.md) | Design docs — exploration, planning, implementation guides |
| `docs/runbook/` | [RUNBOOK-0000](../runbook/0000-runbook-policy.md) | Runbooks — step-by-step operational procedures |

When implementation of a new feature is complete, either:

- Add or update an authoritative spec in `docs/spec/` that describes the actual behavior, or
- Explicitly state in the relevant design doc that it remains a planning reference and that the authoritative spec is the code plus tests.

---

## 7. Summary

| Document type | Purpose | Authority | When to write |
|---------------|---------|-----------|---------------|
| **PRD** | What to build and why (user perspective) | Product intent | Before or at start of new capability |
| **ADR** | Why a specific approach was chosen | Architectural rationale | When a non-trivial decision is made |
| **Spec** | Define behavior; contract for impl and tests | Authoritative; no options | After behavior is decided or implemented |
| **Design** | Explore options; plan work | Not authoritative | Before or during implementation |
| **Runbook** | Step-by-step operational procedure | Operational reference | When an operational task needs documentation |

Specs are the source of truth. We program to the spec. Other document types support the work but do not replace an authoritative spec for shipped behavior.
