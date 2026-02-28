# ADR-0015: Spec-Driven Development

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness is developed with significant AI assistance. As the
codebase grows, maintaining clarity about what the system *should* do
versus what it *happens* to do becomes critical. Without authoritative
documentation, there is a risk that:

- AI-assisted changes drift from intended behavior because there is no
  reference to program against
- Different contributors (human or AI) make contradictory assumptions
  about behavior
- Tests verify implementation details rather than contracted behavior
- Documentation describes aspirations rather than actual system behavior

The project needs a documentation strategy that establishes a single
source of truth for system behavior.

## Decision

Adopt **spec-driven development** as documented in `docs/SPEC_POLICY.md`.

Core principles:

1. **Specs are authoritative.** A spec defines the single correct behavior.
   The implementation must conform to the spec. The spec is not updated to
   match the code after the fact — if behavior needs to change, the spec is
   updated first (or in the same change).

2. **Normative language.** Specs use SHALL/MUST for required behavior and
   MAY for optional behavior. No "could", "might", "we recommend", or
   "implementation may choose".

3. **No options in specs.** A spec states the chosen approach. Alternatives
   and design rationale belong in design docs or ADRs (like this one), not
   in specs.

4. **Implement first, then spec (preferred).** The preferred workflow is to
   implement a feature (or minimal slice), then write the spec describing
   the actual behavior. This ensures specs are concrete and option-free.
   Write-first specs are acceptable when behavior is fully decided in
   advance.

5. **Testable.** Acceptance criteria and tests verify spec compliance. The
   spec is the reference for what "correct" means.

### Document Types

| Type | Purpose | Authority |
|------|---------|-----------|
| **Spec** | Define behavior; contract for implementation and tests | Authoritative |
| **Design / Planning** | Explore options; plan work | Not authoritative |
| **ADR** | Record why a decision was made | Historical record |

### Authoritative Specs

The following documents define the public contract:

- `USAGE.md` — CLI interface and commands
- `SCHEMAS.md` — HTTP API request/response schemas
- `HYBRID_SCORING.md` — Hybrid retrieval algorithm
- `LUA_CONNECTORS.md` — Lua connector scripting interface
- `LUA_TOOLS.md` — Lua tool scripting interface
- `RUST_TRAITS.md` — Rust extension traits
- `AGENTS.md` — Agent system
- `REGISTRY.md` — Extension registry system

## Alternatives Considered

**Code-as-spec (no separate documentation).** The code is the specification.
This works for small projects but makes it difficult to review behavioral
contracts without reading implementation details. It also makes it hard to
distinguish intentional behavior from incidental implementation choices,
especially when AI is generating code.

**RFC process.** A formal Request for Comments workflow where changes are
proposed, discussed, and approved before implementation. Effective for large
teams and open-source projects with many contributors, but heavyweight for a
small-team project. The overhead of RFC review cycles would slow development
without proportional benefit at the current scale.

**Wiki-based documentation.** A wiki (Notion, Confluence, GitHub Wiki) for
living documentation. Wikis tend to drift from reality because there is no
enforcement mechanism tying them to the codebase. Specs in `docs/` are
versioned with the code, reviewed in PRs, and updated in the same commit
as implementation changes.

**Behavior-Driven Development (BDD).** Write specs as executable Gherkin
scenarios. Good for ensuring test coverage matches specs, but adds tooling
complexity (Cucumber, step definitions) and the Gherkin format is verbose
for describing system internals like scoring algorithms or protocol
mappings.

## Consequences

- AI agents (Cursor, Copilot) can be pointed at specs as the authoritative
  reference for behavior, reducing drift and hallucination in generated code.
- Specs are version-controlled alongside the code, ensuring they evolve
  together and can be reviewed in the same PR.
- The "implement first, then spec" preference avoids speculative specs that
  describe unbuilt features. Every spec corresponds to real, working code.
- Normative language (SHALL/MUST/MAY) makes requirements unambiguous and
  testable. There is no guessing about whether a behavior is required or
  optional.
- The spec policy adds a small documentation burden — each feature needs a
  spec update. This is offset by the reduction in ambiguity-related bugs
  and rework.
- ADRs (this directory) complement specs by recording *why* decisions were
  made, while specs record *what* the system does. Together they provide
  both behavioral contracts and historical context.
