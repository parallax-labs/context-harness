# PRD-0009: Retrieval Quality and Dogfooding

**Status:** Planned
**Date:** 2026-02-27
**Author:** pjones

## Problem Statement

Context Harness has shipped a full ingestion and retrieval pipeline, but
it has not been systematically used on real work to identify concrete
retrieval quality issues. Before building new features, the system needs
to be evaluated against real queries on real data to answer:

- Are the right documents surfacing for typical developer questions?
- Is the hybrid alpha well-tuned, or do keyword and semantic signals
  need different weighting?
- Are chunk sizes and boundaries producing good snippets?
- Do the agent prompts work well for real daily workflows?

Without this evaluation, new features risk building on a foundation
whose retrieval quality is unknown. Fixing quality after shipping more
features is harder than fixing it now.

## Target Users

1. **The project maintainer** (dogfooding) who uses Context Harness daily
   to index personal repos, Obsidian vault, and work documentation.
2. **Early adopters** who set up Context Harness and need guidance on
   tuning retrieval for their corpus.
3. **Contributors** who need a regression test suite to ensure changes
   do not degrade search quality.

## Goals

1. Set up a real-world `ctx.toml` indexing 3-5 repos and an Obsidian
   vault used daily.
2. Run 20+ real queries and document where results feel wrong.
3. Use `ctx search --explain` to diagnose each issue (keyword problem,
   semantic problem, or alpha weighting).
4. Tune `hybrid_alpha` and other retrieval parameters based on real
   query data.
5. Write 2-3 real TOML agents for questions asked daily and evaluate
   their prompts.
6. Point Cursor at the MCP server for one week and note friction points.
7. Formalize an evaluation dataset of 20+ query/expected-result pairs
   for regression testing.

## Non-Goals

- Automated benchmark infrastructure (build the dataset first; automation
  can follow).
- Changing the search algorithm (PRD-0001 covers the algorithm; this PRD
  evaluates and tunes it).
- Adding a reranker or cross-encoder (that is Phase 9 / Intelligence
  Layer work, and only if this evaluation shows precision@5 is the
  bottleneck).

## User Stories

**US-1: Identify a bad result.**
The maintainer searches "how does the auth service validate tokens" and
the top result is about token parsing, not validation. They run with
`--explain` and see high keyword overlap but low semantic relevance.
They record this in the eval dataset with the expected correct result.

**US-2: Tune hybrid alpha.**
After collecting 20 queries with expected results, the maintainer runs
each query at alpha values from 0.3 to 0.8 and measures precision@5.
They find alpha=0.5 outperforms the current default of 0.6 for their
corpus and update the config.

**US-3: Evaluate an agent.**
The maintainer creates a `code-reviewer` agent and uses it in Cursor
for a week. They note that the agent's system prompt is too generic
and does not incorporate enough project-specific context. They refine
the prompt and test again.

**US-4: Regression test.**
A contributor changes the chunking algorithm. They run the eval dataset
and verify that precision@5 does not drop below the baseline established
during dogfooding.

## Requirements

1. A real-world `ctx.toml` SHALL be created and used daily for at least
   one week.
2. A minimum of 20 queries SHALL be run and documented with: query text,
   expected top result(s), actual top result(s), and scoring breakdown.
3. `hybrid_alpha` SHALL be evaluated at multiple values and the optimal
   value for the test corpus documented.
4. At least 2 TOML agents SHALL be created and tested in Cursor.
5. An evaluation dataset SHALL be formalized as a structured file
   (JSON or TOML) with query/expected-result pairs.
6. Friction points from one week of Cursor use SHALL be documented.

## Success Criteria

- An eval dataset of 20+ query/expected-result pairs exists and is
  checked into the repository.
- Precision@5 for the eval dataset is documented as a baseline.
- `hybrid_alpha` has been tuned based on data, not just intuition.
- At least 2 agents have been tested in real Cursor sessions and their
  prompts refined.
- A list of retrieval quality issues (with priorities) has been
  documented for future work.

## Dependencies and Risks

- **PRD-0001 (Core Engine):** Must be stable and feature-complete
  before meaningful evaluation.
- **PRD-0004 (Local Embeddings):** Embedding quality directly affects
  semantic search quality. Evaluation should test with the same
  embedding model users will have by default.
- **Subjectivity:** "Good" search results are partially subjective.
  The eval dataset should include clear, unambiguous expected results
  where possible.
- **Corpus specificity:** Tuning parameters on one corpus may not
  generalize. Document the corpus characteristics alongside the
  tuning results.

## Related Documents

- **Specs:** [HYBRID_SCORING.md](../HYBRID_SCORING.md) (scoring
  algorithm being evaluated)
- **PRDs:** [PRD-0001](0001-core-context-engine.md) (core engine),
  [PRD-0004](0004-local-first-embeddings.md) (embedding quality),
  [PRD-0005](0005-observability-and-polish.md) (`--explain` used
  for diagnosis)
