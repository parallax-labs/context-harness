# Design Doc Policy

This document defines how Context Harness uses **design documents**. It applies to all documents in `docs/design/` and governs what a design doc captures, when to write one, how to structure it, how it differs from a spec, and how a design doc graduates into an authoritative spec.

---

## 1. What a design doc is

A **design doc** is a **non-authoritative exploration of how to build something**. It proposes an approach, explores alternatives, identifies risks, and lays out an implementation plan — but it does not define the single authoritative behavior of the system.

- **Exploratory:** A design doc may present multiple options with tradeoffs. It may contain open questions, recommendations, and "TBD" sections. This is expected and encouraged.
- **Planning artifact:** Design docs guide implementation. They describe architecture, data flow, module boundaries, and task breakdowns that help engineers execute.
- **Ephemeral authority:** A design doc is useful during planning and implementation. Once the feature ships, the authoritative contract moves to a spec. The design doc becomes a historical reference.

---

## 2. What a design doc is not

- **Not a spec.** A spec is the single source of truth for behavior. A design doc explores how to achieve that behavior. If a document uses normative language (SHALL, MUST) and claims to be the authoritative contract, it is a spec, not a design doc. See [SPEC-0000](../spec/0000-spec-policy.md).
- **Not an ADR.** A design doc may discuss alternatives, but it is not a frozen record of a decision. ADRs are immutable once accepted; design docs may be updated as understanding evolves. See [ADR-0000](../adr/0000-adr-policy.md).
- **Not a PRD.** A design doc assumes the "what and why" are already defined in a PRD. It focuses on "how." See [PRD-0000](../prd/0000-prd-policy.md).
- **Not a runbook.** A design doc describes how to build a system. A runbook describes how to operate it after it is built. See [RUNBOOK-0000](../runbook/0000-runbook-policy.md).

---

## 3. When to write a design doc

Write a design doc when:

- **The implementation approach is not obvious.** If the feature requires evaluating multiple architectures, choosing between libraries, or designing non-trivial data flows, capture the exploration in a design doc.
- **The work will span multiple PRs or weeks.** A design doc helps coordinate implementation across time and (potentially) contributors.
- **A spec is premature.** If the behavior is not yet fully decided — options remain open, prototyping is needed, or the design may change during implementation — write a design doc first. Upgrade to a spec after the behavior is locked.
- **An implementation plan is needed.** Task breakdowns, ordered steps, and acceptance criteria for a body of work belong in a design doc.

Do not write a design doc for:

- Features where the behavior is already fully decided (write a spec directly).
- Decisions that have already been made (write an ADR).
- Operational procedures (write a runbook).

---

## 4. Design doc structure and required sections

Every design doc MUST include the following metadata and sections:

### Metadata

```
**Status:** Draft | Planning | Reference | Superseded
**Date:** YYYY-MM-DD
**Author:** name or handle
**Related:** Links to PRDs, ADRs, and specs this design supports.
```

### Required sections

| Section | Purpose |
|---------|---------|
| **Context** | What problem or feature does this design address? What constraints apply? Reference the PRD if one exists. |
| **Proposal** | The proposed approach. Include architecture, data flow, module boundaries, and key decisions. Be concrete. |
| **Alternatives Considered** | What other approaches were evaluated and why they were not chosen. Even if brief, this section is mandatory. |
| **Implementation Plan** | Ordered steps or tasks to implement this design. Reference spec sections where applicable. |
| **Open Questions** | Unresolved decisions. Every open question MUST be resolved before the design graduates to a spec. |

### Optional sections

- **Acceptance Criteria:** How to verify the implementation matches this design.
- **Risks:** What could go wrong and how to mitigate it.
- **Dependencies:** What must land first.

---

## 5. Status lifecycle

| Status | Meaning |
|--------|---------|
| **Draft** | Initial exploration. May be incomplete. Open questions are expected. |
| **Planning** | The approach is solidified enough to guide implementation. Open questions are actively being resolved. |
| **Reference** | Implementation is complete. The design doc is a historical reference. The authoritative contract is now a spec (or the code + tests if no spec was written). |
| **Superseded** | A newer design doc or spec has replaced this document. |

Transitions:

- **Draft → Planning:** The proposal is coherent and specific enough to act on.
- **Planning → Reference:** Implementation is complete.
- **Reference → Superseded:** A new design or spec covers the same area with different decisions.

---

## 6. Open questions and the graduation path to spec

Design docs and specs serve different purposes and follow different rules:

| Attribute | Design Doc | Spec |
|-----------|-----------|------|
| Open questions | Permitted and expected | Forbidden (except "TBD" for unimplemented sections) |
| Normative language | Recommendations, proposals | SHALL, MUST, MAY |
| Options | Multiple approaches may be presented | One defined behavior only |
| Authority | Not authoritative | Authoritative — implementation conforms to spec |

### Graduation to spec

When all of the following are true, the design doc should graduate to a spec:

1. All open questions are resolved.
2. The behavior is fully decided — no options remain.
3. Implementation is complete (or the behavior is locked before implementation).

To graduate:

1. Write a new spec in `docs/spec/` using normative language.
2. Mark the design doc as **Reference** or **Superseded**.
3. The spec becomes the authoritative contract; the design doc remains for historical context.

Not every design doc needs to become a spec. If the feature is small or the code + tests are sufficient documentation, the design doc can remain as a Reference without a companion spec. In that case, explicitly note in the design doc that the authoritative contract is the code and tests.

---

## 7. Relationship to other document types

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

- A design doc SHOULD reference the **PRD** that defines the product intent.
- A design doc MAY precede **ADRs** — the design explores options, and the ADR records which was chosen.
- A design doc MAY reference **specs** for behavior that is already locked.
- A design doc is NOT a substitute for a spec. When behavior is authoritative, it belongs in a spec.

---

## 8. Summary

| Attribute | Design Doc |
|-----------|-----------|
| **Purpose** | Explore how to build a feature; plan implementation |
| **Authority** | Not authoritative — informational and planning only |
| **Audience** | Engineers planning or implementing the feature |
| **When to write** | When the approach is non-trivial and a spec is premature |
| **Key rule** | Open questions are expected; resolve them before graduating to a spec |
| **Lifecycle** | Draft → Planning → Reference (or Superseded) |

Design docs are where thinking happens. They are valuable for coordination and context, but they do not replace specs for shipped behavior. When the design is locked and the feature ships, write a spec.
