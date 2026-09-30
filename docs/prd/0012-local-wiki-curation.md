# PRD-0012: Local Wiki Curation from Claude Code

**Status:** Draft
**Date:** 2026-09-30
**Author:** Context Harness contributors

## Problem Statement

A developer uses Claude Code for engineering work and keeps a local Markdown wiki
of project knowledge. Maintaining that knowledge with the paid coding model spends
usage on repetitive summarization, organization and updates. Skipping maintenance
leaves stale knowledge and forces later sessions to rediscover the same facts.

The developer wants Claude Code completion hooks to delegate curation through MCP
to a Context Harness agent running a local model on a Mac M3, initially with Ollama.
The wiki should remain useful and inspectable while reducing paid-model usage to
meet the engineering department's budget expectations. Savings need evidence;
moving work locally is not by itself proof of lower total usage or effort.

## Target Users

- Engineers using Claude Code with an existing local Markdown knowledge base.
- Individual developers with Apple Silicon hardware who want unattended local
  curation without operating a distributed agent platform.
- Engineering teams evaluating usage reductions without sacrificing knowledge
  quality, source attribution or control over their files.

## Goals

1. After meaningful Claude Code work completes, eligible new evidence reaches a
   local curator without another paid-model call to prepare or perform curation.
2. The curator can create and update enrolled wiki content while preserving
   frontmatter, protected sections and human-owned material.
3. Repeated completion notifications do not repeat accepted curation work, and
   concurrent work or interruption does not silently lose user edits.
4. Completed updates are discoverable through Context Harness retrieval, with
   visible status when publication or indexing is incomplete.
5. A representative pilot records paid-model usage per completed task, local
   latency and human repair effort against a baseline before claiming savings.

## Non-Goals

- Replacing Claude Code's engineering model or automatically falling back to a
  paid provider when local inference fails.
- General autonomous source-code modification, shell access or unrestricted
  rewriting of the user's wiki.
- Supporting multiple local inference servers in the first delivery; llama.cpp
  remains a possible later option.
- Distributed workers, remote multi-user execution, analytics infrastructure,
  or a guaranteed savings percentage before measurement.
- Implementing this curation feature in PR #33. That PR contains the existing
  runtime foundation plus the documentation for this future capability.

## User Stories

- After Claude completes relevant engineering work, I want the local curator to
  extract durable knowledge with source references so I do not spend paid-model
  usage maintaining the wiki manually.
- When the same completion event is delivered twice, I want one curation result
  rather than duplicate pages or repeated inference.
- When I edit a wiki page while curation is running, I want my edit preserved and
  any conflict surfaced for review.
- In a later Claude session, I want concise, current wiki results through MCP
  rather than loading the entire knowledge base into every prompt.
- If Ollama is unavailable, I want engineering work to continue, a clear local
  failure status, and no unexpected cloud-model call.
- Before enabling automation on my real wiki, I want to inspect proposed changes
  in a disposable pilot and explicitly choose the content it may maintain.

## Requirements

| ID | Product requirement |
|---|---|
| W1 | Connect Claude Code completion events to a configured Context Harness curator through MCP without asking the paid model to orchestrate routine curation. |
| W2 | Run curation inference locally using Ollama on the user's Mac; local unavailability is visible and never silently uses a paid fallback. |
| W3 | Limit reads and updates to enrolled evidence and wiki content; users choose what is automatically maintained and what stays protected. |
| W4 | Support bounded Markdown page creation and updates with provenance; preserve existing frontmatter and human-owned sections. |
| W5 | Suppress duplicate and recursive work, coordinate concurrent sessions, and surface conflicting or interrupted changes without silently overwriting them. |
| W6 | Make successfully curated content available to retrieval; distinguish a published file from a fully refreshed search index. |
| W7 | Let users inspect outcomes, failures and pending proposals, and disable the integration without breaking normal Claude Code or existing Context Harness prompts. |
| W8 | Bound local work and the context returned to Claude; measure usage reduction alongside knowledge quality and repair effort. |

## Success Criteria

- The end-to-end disposable-wiki test completes from a Claude event through local
  curation and retrieval with zero paid inference calls attributable to curation.
- Duplicate/recursive event tests produce no duplicate accepted work or runaway
  processing; concurrent-edit and restart tests lose no protected user content.
- Every accepted knowledge change has evidence references, and every protected
  section/frontmatter fixture remains intact. Unsupported claims in the evaluation
  set are rejected or retained as proposals for review.
- Once a job reports full completion, new/updated knowledge is retrievable; indexing
  failures remain visible and can be retried without repeating curation.
- Local-provider failure leaves Claude usable and produces a diagnosable outcome.
- Before real-wiki activation, the user approves the enrolled scope and pilot quality.
  Before declaring budget success, agree on a numerical usage target and evaluate
  it over comparable completed tasks, reporting latency and human corrections too.
  Unavailable measurements remain unknown; subscription savings are not inferred
  directly from token reductions.

## Dependencies and Risks

- Depends on the runtime foundation in PR #33 and a subsequent authoritative spec
  for executable MCP curation, scoped publication and failure/retry behavior.
- The selected local model needs adequate memory and reliable tool/structured-output
  behavior. Mac M3 chip identity alone does not establish model capacity or quality.
- Claude completion events can occur before an entire engineering task is complete;
  filtering and installed-version compatibility need validation.
- Existing mixed-ownership Markdown files can race with noncooperating editors.
  Automatic writes need an explicit ownership convention; other changes can remain
  proposals rather than promising conflict-free overwrites.
- Evidence can contain sensitive or misleading text. Scope, retention and provenance
  controls need agreement before real project data enters unattended curation.
- Additional context or frequent notifications can increase paid usage even if
  curation itself is local. Measure the whole workflow.

## Open Questions

Resolve before this PRD becomes Planned/In Progress:

1. Which wiki/project roots, page conventions and managed-content boundaries are
   enrolled, and which existing pages remain proposal-only?
2. What unified memory and locally installed Ollama model are available for the pilot?
3. Which Claude Code version and completion signal will the integration use?
4. What evidence retention and sensitive-data rules apply?
5. What baseline workload, quality bar, usage-reduction target and acceptable latency
   define success for the engineering budget requirement?

## Related Documents

- [DESIGN-0012](../design/0012-local-wiki-curation.md): architecture alternatives,
  proposed flow, implementation slices and acceptance scenarios for W1–W8.
- [DESIGN-0010](../design/0010-local-agent-runtime.md) and
  [DESIGN-0011](../design/0011-local-agent-runtime-execution-plan.md): existing
  runtime foundation, not delivery claims for this use case.
- [ADR-0024](../adr/0024-local-agent-runtime.md): existing optional runtime decision;
  this PRD does not amend that accepted record.
- Feature-specific ADRs: deferred until the design choices are settled.
- Feature-specific spec: next documentation stage after design review and resolution
  of behavioral choices; no placeholder authoritative spec is published here.
- Setup/operation runbook: subsequent implementation deliverable once behavior is
  implemented and verified.

Current sequence: PRD and design in PR #33; review and resolve the design, then
create the spec; user tests and merges the existing runtime; subsequent work
implements wiki curation against the agreed spec. No curation implementation or
real-wiki activation is authorized by this document alone.
