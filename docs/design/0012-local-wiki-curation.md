# DESIGN-0012: Local Wiki Curation from Claude Code

**Status:** Planning
**Date:** 2026-09-30
**Author:** Context Harness contributors
**Related:** [PRD-0012](../prd/0012-local-wiki-curation.md), [runtime design](0010-local-agent-runtime.md), [runtime execution plan](0011-local-agent-runtime-execution-plan.md), [execution](../spec/0017-local-agent-execution.md), [policy](../spec/0018-developer-tools-and-approvals.md), [recovery](../spec/0019-checkpoints-recovery-and-artifacts.md), [prompt projection](../spec/0022-resource-prompt-projection.md), [PR #33](https://github.com/parallax-labs/context-harness/pull/33)

## Context

The target user runs Claude Code for engineering work on a Mac M3 and maintains
local Markdown wiki files. After useful work completes, a Context Harness agent
should curate that wiki using Ollama locally. Claude Code accesses the integration
through MCP. The objective is to reduce paid-model curation and repeated context
reconstruction while retaining useful, attributable knowledge.

PR #33 supplies durable runs, bounded execution, permissions, and static MCP prompt
projection. It does not yet expose executable runtime agents over MCP, configure a
local inference provider, authorize unattended wiki writes, serialize curation,
or refresh retrieval after filesystem updates. Completion of DESIGN-0011 means
runtime infrastructure acceptance, not acceptance of this end-user workflow.

This design supports [PRD-0012](../prd/0012-local-wiki-curation.md), which defines
product requirements W1–W8. PR #33 adds documentation only for this use case; its
existing runtime implementation remains available for user testing. Proposed
commands, configuration and tools below do not exist yet. The supplied DESIGN-0010
remains unchanged.

### Document chain and delivery boundary

1. **Product intent:** PRD-0012 (Draft) captures the workflow, budget objective,
   requirements and success measures.
2. **Design:** this document (Planning) explores the approach and alternatives.
   Review its open questions before committing to a behavioral contract.
3. **Decisions and spec:** record settled non-trivial choices in new ADRs, then
   write the feature spec with exact behavior and testable acceptance criteria.
   The spec is the next documentation stage; it is deliberately not created here.
4. **Test and merge the foundation:** the user tests the runtime already in PR #33
   and merges it. Wiki curation remains unimplemented in that PR.
5. **Subsequent implementation:** deliver the slices below in follow-up work against
   the agreed spec, followed by a verified setup/operation runbook and local pilot.

Existing SPEC-0015 through SPEC-0022 describe the runtime foundation. They do not
serve as the spec for executable wiki curation. Design review and foundation testing
may proceed independently; implementation waits for both the agreed spec and the
user's test/merge handoff.

## Proposal

### End-to-end path

```text
Claude Code completion event
    -> deterministic hook selects new evidence and a stable request key
    -> MCP wiki.curate accepts a bounded request and returns a job ID
    -> SQLite-backed worker serializes jobs for that canonical wiki
    -> Context Harness agent calls local Ollama and reads scoped evidence
    -> agent proposes Markdown changes; host validates and publishes them
    -> host reindexes only changed wiki documents
    -> wiki.status exposes outcome and a compact summary

Claude Code search/get -> indexed wiki snippets when relevant
```

Keep existing MCP prompts stateless and compatible. Add explicit executable tools;
do not change `prompts/get` into a runtime call. Initial tool names are proposed as
`wiki.curate` and `wiki.status`, with only the configured curator exposed. Do not
allow a request to select an arbitrary agent, model endpoint, filesystem root,
process command, or more permissive policy. Mark the submission tool as mutating;
the bridge currently labels all tools read-only and needs accurate annotations.

### Local model boundary

Implement an Ollama provider first, preferably its native non-streaming chat API,
reusing ModelRequest/ModelResponse and the existing registry. Keep llama.cpp as a
future provider, not a second implementation in the initial slice. Configure an
explicit local model name, loopback endpoint, per-request timeout, context/output
limits and optional authentication without recording secrets. Validate URL scheme,
loopback destination and redirects; disallow cloud fallback. Verify the selected
model is locally installed rather than an Ollama cloud model. Local-only operation
is a property to test, not an inference from a localhost URL.

Map tool calls/results, finish reasons and usage; reject malformed or unsupported
responses. Bound HTTP bodies, history and individual tool results. Preserve absent
usage as unknown. Test against fixtures and then the actual Mac/model. Choose model
size/quantization after checking available unified memory and a small curation eval;
M3 alone is not enough information. Account for cold model loading separately from
normal inference timeouts. Do not fetch model weights implicitly during a job.

### Host-owned wiki authority

Add a trusted integration configuration that binds a stable wiki ID to its
canonical root, curator resource, allowed source roots, local model and limits.
The integration policy grants unattended access only to selected wiki operations;
resource declarations can narrow that authority but cannot widen it. Keep generic
CLI non-interactive approval denial unchanged. No shell/process, arbitrary network,
external MCP startup, or child-agent delegation is required for the initial curator.

Prefer narrow `wiki.read`, `wiki.search` and `wiki.propose` tools. Pass small,
immutable evidence snippets in the initial input; for larger accepted evidence,
provide a read-only `evidence.get` tool keyed by job-scoped evidence IDs. It reads
only captured submission bytes, never arbitrary caller/model-selected paths. The model returns
a bounded change set; deterministic host code validates and publishes it. First
release supports create/update of Markdown pages and managed index sections. It
does not delete or rename pages, edit source code, or alter Claude instructions.
Preserve frontmatter and human-owned sections; initial automatic updates are
restricted to explicitly marked managed sections. Treat existing unmarked content
as read-only until the user deliberately enrolls it. Never silently rewrite it.

Use descriptor-relative path handling where practical, reject symlink/hardlink
escapes and protected paths, restrict extensions and enforce file/change-set limits.
Each proposal identifies its source evidence and expected original content hash.
Validate against a fresh read before publication. Model output, wiki text and
transcripts are untrusted data, not authority to change configuration or permissions.

### Jobs, duplicate suppression and publication

Use application-level SQLite tables for jobs, request keys, attempts, change sets,
publication records and indexing status. Do not move operational events to DuckDB
or Parquet. Acceptance records and request-key uniqueness commit transactionally.
Persist each job attempt
and its runtime run ID; distinguish a new inference attempt from publication
reconciliation and indexing-only retries in status/history.
A caller request key identifies one immutable input digest: an exact duplicate
returns its existing job; reuse with different input returns a conflict. Bind all
keys and reads to the configured wiki/workspace identity.

Run one curator per canonical wiki root, including across two server processes;
per-run locks are insufficient. Store jobs durably and use an OS-held wiki lock plus
explicit restart reconciliation. Limit queue depth, attempts, time, model turns,
input/output size, changed pages and accepted work per time window. Use a bounded hook-side debounce before submission. Defer merging accepted
jobs until pilot evidence justifies the extra identity/recovery complexity. Mark
obsolete queued snapshots explicitly rather than silently changing their input.

Publish each file using a sibling temporary file, flush and atomic replacement,
with an expected-content-hash check and recoverable before/after hashes. Such a
check plus rename does not provide true compare-and-swap against arbitrary editors.
The first unattended target should therefore be curator-owned pages/managed content
under an explicit ownership convention. If user-editable mixed pages cannot be
protected by a cooperating lock, retain a proposal for review instead of promising
race-free automatic overwrite. Never claim multiple file renames form one atomic
transaction. A publication journal makes partial batches and restart recovery
visible and allows reconciliation against actual hashes without blindly replaying.

Persist states such as queued, running, publishing, indexing, completed, failed,
conflict and reconciliation_required (final names/schema set in the implementing
spec). Expose content publication and index freshness separately. A retry after
publication should retry indexing, not invoke the model or rewrite files again.
An ambiguous crash during publication requires reconciliation. Automatic recovery
of ordinary runtime trees/external sessions remains outside this work.

### Retrieval freshness

Reuse filesystem ingestion, normalization/chunking and canonical SQLite/FTS storage
for changed wiki documents. Refresh only after successful publication, using stable
source/document identities; preserve unrelated indexed data. Default this curation
path to keyword retrieval so it cannot silently trigger paid embedding calls.
If embedding is enabled later, require a separately configured local provider.

Report `published_pending_index` or equivalent when indexing fails; do not report
full success while retrieval is stale. An indexing retry is idempotent. Bind MCP
search/get to the intended wiki source; do not let curation requests cross workspace
boundaries. Test that new pages are immediately retrievable after a completed job
and that updated pages no longer return superseded indexed content.

### Claude Code hooks and budget

Provide a version-checked setup example and a deterministic hook adapter. Current
Claude Code documentation supports MCP tool hooks; a command-hook MCP client is a
fallback for versions without the needed integration. Use direct tool submission,
not a prompt/agent hook that asks a paid model to summarize or decide what to curate.
The fast MCP response contains only an accepted job ID/status; inference runs in
the local worker. Status retrieval is explicit or once at a later useful boundary,
not a repeated cloud-model polling loop.

`Stop` fires at response completion, not guaranteed engineering-task completion.
Initially filter it for new relevant evidence, guard `stop_hook_active`, and use
stable session/turn/evidence keys. Debounce and skip no-op events. Ignore curator
writes and curator tool calls so updates cannot trigger themselves. A task-complete
signal may be used when the installed Claude Code workflow supplies one, but do
not assume every session uses task tracking. Hook failures should not block Claude
or instruct it to retry endlessly; retain a visible local failure record.

Select bounded evidence locally: changed source paths/hashes, relevant new
transcript segments and task outcome. Avoid full-transcript resubmission each turn.
Never interpolate hook strings into shell commands or trust arbitrary incoming
transcript paths; constrain sources to configured roots and reject oversized input.
The wiki root can differ from the project root. Capture evidence at submission or
validate its digest before processing so a queued job cannot curate different bytes.

Keep the full wiki out of always-loaded Claude context. Return short search snippets
with source references. Measure paid Claude input/output/cache usage per comparable
completed task, local curation latency, duplicate/no-op rate and accepted edit quality.
Report costs only with explicit applicable pricing; distinguish token/usage quota
reduction from subscription dollars. Missing measurements are unknown, never zero.
No savings percentage is promised before a baseline and pilot exist.

## Implementation Plan

The following slices are a future implementation plan, not work to add to PR #33.
After design review, create the authoritative spec before implementation; after the
user tests and merges the foundation, deliver these slices in follow-up work. Each
slice includes spec-linked acceptance tests and a reviewable commit. Do not activate
unattended hooks against the user's real wiki until the pilot gate passes.

| Slice | Work / likely modules | Acceptance gate | Status |
|---|---|---|---|
| 1. Local inference | `agent_model/ollama.rs`, registry/config and provider tests | Fixture multi-turn tool loop, malformed responses, unavailable model, context bounds, timeout and no cloud fallback; live local smoke on Mac | Pending |
| 2. Wiki scope and proposals | Host integration config; dedicated wiki tools and change-set validator | Wiki-only access; create/update; frontmatter and human sections preserved; forbidden paths/tools denied; cited evidence; invalid output causes no writes | Pending |
| 3. Durable jobs and publication | Application migrations/store; worker and wiki lock; publication journal | Duplicate event returns same job; key/input conflict rejected; two workers serialize; bounded queue; concurrent edits become conflicts; crash injection across file/DB boundaries reconciles safely | Pending |
| 4. Executable MCP surface | Narrow submit/status tools and accurate annotations; server lifecycle | Submission returns promptly; configured wiki/agent only; durable job survives disconnect/restart; missing/invalid tokens and cross-scope submit/status rejected; hook transport authentication verified; existing prompts and multi-workspace contracts unchanged | Pending |
| 5. Fresh retrieval | Targeted filesystem sync and retry state | Create/update visible through MCP search/get; indexing failure visible and retryable without another model call; no paid embedding path | Pending |
| 6. Claude integration | Hook adapter, sample settings, curator resource, setup/disable runbook | Repeat/recursive/no-op events suppressed; bounded immutable evidence; no paid hook model; failure does not block Claude; version compatibility checked | Pending |
| 7. M3 pilot and budget evaluation | Fixture wiki, live Ollama eval, usage comparison | End-to-end local curation meets quality/correctness gates and measured usage target; enable real wiki only after explicit scope configuration | Pending |

Dependencies: 1 and 2 establish model/tool contracts; 3 depends on 2, 4 on 1–3,
5 on 3, 6 on 4–5, and 7 exercises the integrated system. Begin with read-only/proposal
mode; enable curated-page publication only after the mutation and recovery gates.
Maintain a small rejection set (unsupported claims, missing provenance, duplicate
knowledge, contradictory evidence, malformed frontmatter and instruction injection)
alongside successful curation examples.

### End-to-end acceptance scenario

1. In a disposable project/wiki, Claude completes a change with documented evidence.
2. The hook submits it twice; both submissions identify the same curation job.
3. Local Ollama curates a new page and an allowed managed section, with source links.
4. A simultaneous second session queues work; it cannot publish over the first job.
5. A human edit before publication produces a conflict, not a lost edit. Mixed-page
   noncooperating editor limitations are exercised in proposal-only mode.
6. Inject a crash after file publication but before indexing; restart reconciles the
   journal and reindexes without regenerating or duplicating content.
7. MCP retrieval sees published content; status summarizes changed pages and errors.
8. Local-provider unavailability never calls a paid provider. Verify inference network
   destinations, queue behavior and hook nonblocking behavior.
9. Compare representative Claude tasks before/after, checking useful knowledge,
   paid usage, local latency and human repair effort. Set numerical acceptance
   thresholds from the baseline before claiming a budget win.

### Product requirement traceability

| PRD requirement | Design area | Planned slices |
|---|---|---|
| W1: completion-to-MCP delegation | End-to-end path; Claude Code hooks | 4, 6 |
| W2: local inference, no paid fallback | Local model boundary | 1, 7 |
| W3: enrolled content and authority | Host-owned wiki authority | 2, 4 |
| W4: Markdown updates and provenance | Proposals and publication | 2, 3 |
| W5: duplicates, conflicts and recovery | Jobs and publication journal | 3, 6 |
| W6: fresh retrieval | Retrieval freshness | 5 |
| W7: inspect/disable without disruption | MCP status; hook failure handling | 4, 6 |
| W8: bounded work and measured savings | Model/job limits; budget pilot | 1, 3, 7 |

The subsequent spec should retain W1–W8 references and assign precise behavioral
requirements to each acceptance test. Resolve authentication, file ownership,
retry semantics and numerical bounds before declaring that spec authoritative.

## Alternatives Considered

- **Only expose MCP prompts:** retains compatibility but leaves execution and token
  expenditure with Claude. Keep it as a separate existing interface.
- **Wrap `ctx agent run` in every hook:** useful for an early provider smoke test,
  but does not solve durable deduplication, scoped unattended writes or safe retries.
- **Give the agent shell access:** can create files quickly but exceeds the required
  authority. Narrow wiki proposals plus deterministic publication are preferable.
- **Generic runtime submission API/queue:** broader surface than this use case needs.
  Start with one configured curator; internal job primitives can generalize later.
- **llama.cpp first / support both immediately:** plausible later option; Ollama is
  the user's initial choice. Validate one full provider path before multiplying it.
- **DuckDB, external brokers or analytics infrastructure:** unnecessary for this
  delivery. SQLite remains canonical; basic metrics suffice for the pilot.

## Open Questions

Resolve these before the pilot and promotion to an authoritative spec:

1. Exact wiki and evidence roots, page layout, frontmatter conventions, protected
   sections, and whether curator-owned pages are acceptable for unattended writes.
2. M3 unified memory, installed Ollama version and a locally installed model that
   passes the tool-calling and curation eval. No model download is authorized here.
3. Installed Claude Code version and completion signal; confirm MCP hook availability
   and choose `Stop` filtering or an explicit task-complete event accordingly.
4. Source/transcript retention, redaction rules and whether evidence may include
   engineering secrets. Do not add retention/deletion behavior silently.
5. Initial numeric input/context/output, queue, page-change and time limits, plus
   quality and paid-usage reduction thresholds measured over representative tasks.
6. Executable MCP authentication: use loopback-only serving with a per-install token
   for the initial mutating surface. Define token storage/rotation and compatibility
   with existing read-only clients before enabling execution; remote access is deferred.

## References

- [Claude Code hooks](https://code.claude.com/docs/en/hooks): lifecycle, MCP tool hooks,
  command hooks and recursion handling. Verify installed-version support in slice 6.
- [Ollama tool calling](https://docs.ollama.com/capabilities/tool-calling): initial
  provider protocol reference; verify against the installed server in slice 1.

This document is a planning reference. New behavior becomes authoritative only
through its implementation, acceptance tests and corresponding specifications.
