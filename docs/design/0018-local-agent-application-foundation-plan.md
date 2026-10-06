# DESIGN-0018: Local Agent Application Foundation Implementation Plan

**Status:** Draft
**Date:** 2026-10-02
**Author:** Context Harness contributors
**Related:** [PRD-0014](../prd/0014-durable-agent-runs.md), [DESIGN-0014](0014-structured-agent-run-state.md), [PRD-0015](../prd/0015-local-generation-providers.md), [DESIGN-0015](0015-local-generation-providers.md), [PRD-0016](../prd/0016-durable-background-agent-tasks.md), [DESIGN-0016](0016-durable-background-agent-tasks.md), [PRD-0017](../prd/0017-targeted-document-refresh.md), [DESIGN-0017](0017-targeted-document-refresh.md), [ADR-0024](../adr/0024-local-agent-runtime.md), [ADR-0026](../adr/0026-native-ollama-local-generation.md), [ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md), [SPEC-0016](../spec/0016-model-runtime.md), [SPEC-0017](../spec/0017-local-agent-execution.md), [SPEC-0019](../spec/0019-checkpoints-recovery-and-artifacts.md), [SPEC-0023](../spec/0023-declarative-tool-bindings.md), [SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md)

## Context

PRD/DESIGN-0014 through 0017 describe four related but independently useful areas:

1. durable long-running runs with typed outcomes, cumulative budgets, structured
   working state and bounded context projection;
2. local generation providers and a trusted provider registration seam;
3. durable background task submission and independently managed workers;
4. targeted refresh of known changed documents through the canonical ingest path.

Together they form the generic Context Harness foundation required by local
applications that accept work, run bounded agents, publish their own results, and
refresh retrieval. They do not define any application's hooks, evidence, files,
publication rules, or domain tools.

The four designs have different dependency shapes. Local generation and targeted
refresh can proceed independently. Background task execution needs authoritative
run lifecycle and recovery semantics, but it does not need context compaction or a
complete structured-working-state implementation. Long-run context work can continue
after the minimum lifecycle contract is available.

This document coordinates the work across designs. It does not settle their open
questions or replace the ADRs and specs required before behavioral implementation.

## Proposal

### Delivery principles

1. **Contracts before behavior.** Resolve an area's open public-behavior questions in
   an ADR/spec before implementing that behavior.
2. **Compatibility seams before replacement.** Introduce provider, context-builder,
   ingestion and host-assembly seams with existing behavior first.
3. **One invariant per pull request.** Schema, provider, lifecycle, worker, context,
   and ingestion changes remain independently reviewable.
4. **Vertical validation.** Each track ends in an ordinary non-domain fixture that
   proves the public behavior through CLI or public Rust APIs.
5. **No domain dispatch.** Application names, hook formats, publication policies and
   wiki semantics never enter the runtime.
6. **Conservative recovery.** No phase weakens the existing rule against replaying
   uncertain side effects.
7. **Side-effect-free inspection.** Static list/show/validate never opens the run
   database, contacts providers, starts workers, evaluates Lua, starts MCP, or
   executes tools.
8. **SQLite remains canonical.** Provider continuations, derived context and vector
   indexes remain bounded or rebuildable around canonical records.

### Dependency graph

```text
Phase 0: decisions and authoritative contracts
   ├── lifecycle/outcome/recovery contract (0014)
   ├── provider catalog + Ollama contract (0015)
   ├── task/run/lease contract (0016; depends on lifecycle contract)
   └── targeted refresh contract (0017)

Phase 1: compatibility seams
   ├── RunContextBuilder preserving current requests
   ├── ModelProviderCatalog preserving OpenAI/fake
   ├── shared per-item ingestor preserving full sync
   └── public AgentHost assembly preserving direct runs

Phase 2: independent vertical capabilities
   ├── Ollama local generation
   └── targeted create/update refresh

Phase 3: minimum durable-run lifecycle
   ├── typed outcomes and reasons
   ├── cumulative budgets and inspection
   └── checkpoint/migration compatibility

Phase 4: durable background tasks
   ├── task store and idempotent submission
   ├── worker claims, leases and cancellation
   └── task-to-run recovery reconciliation

Phase 5: extended long-running behavior
   ├── structured working state
   ├── bounded context selection/compaction
   ├── classified recoverable observations
   └── blocked/needs-input continuation

Phase 6: integrated hardening and application handoff
```

Phase 1 tracks can run in parallel after their individual contracts are sufficiently
clear. Phase 2 provider and ingestion tracks do not wait for Phase 3. Phase 4 waits
for the Phase 3 lifecycle/recovery subset, not for Phase 5.

### Pull-request sizing rules

Each implementation pull request should:

- have one primary design slice and one named invariant;
- include its migration and compatibility tests when it changes persistence;
- avoid combining schema migration, new provider behavior, worker behavior and
  context compaction in one review;
- retain existing behavior behind a compatibility implementation when introducing a
  seam;
- update the authoritative spec in the same pull request when implementation changes
  specified behavior;
- update the originating design's slice status only after validation passes;
- include exact commands and results for formatting, all-features Clippy, workspace
  tests, targeted integration tests and `git diff --check`.

## Implementation Plan

### Phase 0: Resolve contracts and freeze cross-track boundaries

No runtime behavior changes in this phase. Documentation changes may be separate PRs
so each decision is reviewable.

#### 0A. Durable-run lifecycle contract

Resolve the DESIGN-0014 questions required by background work:

- typed outcome versus lifecycle status versus termination reason;
- terminal, suspended and resumable states;
- completion/control mechanism;
- cumulative token/tool/time budget semantics;
- cancellation and uncertain-side-effect representation;
- legacy row, checkpoint and CLI JSON compatibility.

Deliver an ADR for the chosen run-state/context boundary and revise SPEC-0017 and
SPEC-0019, or add one focused spec if that produces a clearer authority boundary.
Context compaction details may remain unresolved if the initial context-builder
compatibility behavior is specified.

**Gate L:** Background worker reconciliation may not implement outcome projection
until typed run outcomes and recovery categories are authoritative.

**Gate status (2026-10-03): Resolved.**
[ADR-0028](../adr/0028-typed-run-outcomes-and-compatibility.md) selects separate
canonical lifecycle, outcome, reason, and recovery concepts with a four-value legacy
status projection. [SPEC-0025](../spec/0025-durable-run-lifecycle-and-budgets.md)
defines typed outcomes, opt-in control calls, cumulative budgets, unknown token-usage
fallback, inspection, migration, and recovery dispositions. Phase 3 may implement
that contract; working state, compaction, recoverable observations, and suspended
continuation remain Phase 5 work.

#### 0B. Local-provider contract

Resolve native Ollama versus OpenAI-compatible protocol, provider-specific config,
local-only endpoints, redirects, capability reporting, identity, cancellation and
readiness. Record the initial protocol choice in an ADR and update SPEC-0016.

**Gate P:** The provider catalog seam may preserve behavior before this decision;
the production local adapter waits for it.

**Gate P complete (2026-10-02):** [ADR-0026](../adr/0026-native-ollama-local-generation.md)
selects native non-streaming Ollama chat over literal-loopback HTTP with redirects
disabled and no fallback, model download or readiness side effects. The revised
[SPEC-0016](../spec/0016-model-runtime.md) fixes provider fields, identity, message and
tool mapping, structured output, bounds, errors, cancellation and offline inspection.

#### 0C. Background-task contract

After Gate L, specify task identity, request-key digest, scheduling states, accepted
resource identity, drift handling, claim/lease semantics, attempts, cancellation,
task-to-run linkage, reconciliation and public inspection. Task state describes only
scheduling and ownership; the linked run remains authoritative for execution outcome.

**Gate Q:** Persistent task migrations and public CLI behavior wait for this spec.

**Gate status (2026-10-05): Resolved.**
[ADR-0029](../adr/0029-durable-agent-task-ownership.md) and
[SPEC-0026](../spec/0026-durable-agent-tasks.md) define the separate task/run
authority boundary, accepted identity, idempotent submission, four scheduling states,
one immutable run link, database-clock claims and leases, bounded worker settings,
cancellation, conservative reconciliation and staged public API/CLI delivery. Phase
4A may implement persistence and submission without worker behavior.

#### 0D. Targeted-refresh contract

Resolve input form (`SourceItem`, connector item ID, or both), enrolled source handle,
batch limits, transaction boundary, outcome taxonomy, optional embedding/sidecar
failure and initial library/CLI scope. Update the ingestion/usage specs or add a
focused targeted-refresh spec.

**Gate I:** Internal extraction may begin with equivalence tests; the public targeted
API waits for this contract.

**Gate status (2026-10-03): Resolved.** [ADR-0027](../adr/0027-targeted-refresh-authority-and-atomicity.md)
and [SPEC-0024](../spec/0024-targeted-document-refresh.md) define the trusted
enrolled-source authority, `SourceItem` input, bounded preflight, per-document
canonical transaction, derived-work outcomes, checkpoint isolation and initial
library-only scope. Phase 2B may expose the public targeted API against that contract.

### Phase 1: Introduce compatibility seams

These slices should change structure without changing observable behavior.

#### 1A. Context projection seam

Extract `RunContextBuilder` from request construction. The compatibility builder
reproduces the current valid system/user/assistant/tool transcript and tool schema.
Record non-secret projection metadata for tests without adding compaction or working
state.

**Likely modules:** `agent_runtime`, `agent_model`, checkpoint fixtures.

**Acceptance:** existing runtime and resume fixtures produce equivalent requests and
outcomes; no new lifecycle state or database migration.

**Slice status (2026-10-05): Implemented.** The runtime now delegates fresh and
restored request projection to an internal `RunContextBuilder`. The compatibility
builder preserves the complete transcript and existing tool declarations verbatim,
while emitting non-secret builder, strategy, category and count metadata into
context-resolution events and checkpoint schema v2. No compaction, selection policy,
working-state model or public lifecycle change is introduced.

#### 1B. Provider catalog seam

Move OpenAI/fake construction out of provider-name branching into a trusted provider
factory catalog. Add deterministic identity/collision behavior and external compiled
registration without provider calls during validation.

**Slice status (2026-10-02): Implemented.** `ModelProviderCatalog` now owns all
provider-name dispatch, registers the existing OpenAI and fake implementations through
the same factory interface, rejects duplicate provider names without replacement, and
exposes stable non-secret implementation ID/version metadata. A public fixture factory
proves trusted compiled host registration. Catalog validation validates definitions and
factory-specific settings without constructing providers, resolving credentials, opening
the database, starting processes, or making model/network calls. Existing registry
identity and checkpoint bindings remain unchanged; consuming implementation metadata in
checkpoint compatibility is deferred to a versioned checkpoint slice.

**Likely modules:** `agent_model`, `agent_resource`, config inspection, crate exports.

**Acceptance:** OpenAI/fake tests remain unchanged in behavior; a fixture provider
registers through public APIs; static inspection stays offline.

#### 1C. Shared document ingestor

Extract extraction, canonical upsert, chunk replacement, inline embedding and vector
sync from connector orchestration into one per-item service. Continue calling it only
from full sync initially.

**Likely modules:** `ingest`, `embed_cmd`, `vector_index`, `app_store`.

**Acceptance:** before/after full-sync fixtures produce equivalent canonical rows,
chunks, checkpoints, progress totals and search results across embedding modes.

**Slice status (2026-10-02): Implemented.** Full connector sync now delegates each
filtered item to one crate-private document ingestor for extraction, canonical upsert,
chunk replacement, inline embedding and configured vector-index synchronization.
Connector discovery, filters, dry-run behavior, progress, console summaries and
checkpoint advancement remain in the full-sync orchestrator. Characterization and
existing integration coverage preserve replacement, checkpoint, extraction, search,
custom-connector and embedding behavior. No targeted-refresh API, public outcome
taxonomy or new checkpoint semantics are introduced.

#### 1D. Public agent-host assembly

Consolidate the supported construction path for resolved config, canonical workspace,
stores, model catalog, tool implementation catalog, runtime registry and host
authority. Direct execution is the only behavior initially.

**Slice status (2026-10-02): Implemented.** `AgentHostBuilder` is the public,
direct-execution assembly path for resolved configuration, a canonical workspace,
agent resources, a trusted `ModelProviderCatalog`, optional trusted tool factories and
explicit `HostToolAuthority`, runtime policy, approvals and the existing durable run
store. The CLI run and resume paths use the same builder. Builder configuration is
inert, and assembly performs no model invocation, tool execution, credential
resolution or worker startup. The default builder grants no capabilities and installs
no tool authority; the CLI opts into its existing read-only authority and runtime
policy explicitly. A public-API fixture composes external model and tool factories and
proves direct run behavior without adding task semantics, lifecycle states or schema
changes.

**Likely modules:** new host/builder module, `agent_runtime`, `tool_binding`, crate
exports, one external fixture.

**Acceptance:** the CLI and fixture host construct the same effective runtime; no
default authority, worker, provider call or tool execution occurs during assembly.

### Phase 2: Deliver independent vertical capabilities

Provider and ingestion work may proceed in parallel.

#### 2A. Ollama generation provider

Implement typed configuration and native protocol mapping for messages, tools,
results, output limits, finish reasons, structured output where supported, usage,
timeouts and cancellation. Enforce local-only endpoint/redirect policy and prohibit
fallback or implicit model download.

Split if necessary:

1. configuration, identity and static inspection;
2. HTTP protocol mapping and fixture tests;
3. local-only policy, cancellation and bounded bodies;
4. explicit readiness command and opt-in live smoke test.

**Acceptance:** a generic agent completes `search -> get -> final answer` locally;
unavailable model and protocol errors are distinct; network observation finds no
fallback request.

**Slice status (2026-10-02): Implemented.** The built-in catalog now constructs the
native Ollama chat adapter from typed, statically validated configuration. Fixture
coverage exercises the protocol mapping, local-only transport policy, bounded bodies,
timeouts, cancellation and classified failures, and an ordinary `AgentRuntime` test
completes `search -> get -> final answer`. Readiness, model download and live quality
evaluation remain deferred.

#### 2B. Targeted create/update refresh

Add host-issued enrolled source scope, bounded batch validation, structured per-item
outcomes and public create/update APIs over the shared ingestor. Do not advance scan
checkpoints. Exclude deletion.

Split if necessary:

1. enrolled source handle and validation;
2. library create/update API and structured outcomes;
3. embedding/vector partial-failure and retry behavior;
4. optional CLI only if an operator workflow is approved.

**Acceptance:** known items refresh without `Connector::scan`; superseded chunks
disappear; unrelated content and checkpoints remain unchanged; identical retries are
idempotent.

**Slice status (2026-10-03): Implemented.** `EnrolledSource` provides opaque,
workspace-bound authority for one configured source, and `TargetedRefresher` exposes
bounded public-library create/update refresh with whole-batch preflight and ordered
per-item canonical, embedding and sidecar outcomes. Canonical document/chunk/FTS and
prior-vector replacement is atomic per item; derived failures remain independently
retryable after canonical commit. Tests demonstrate offline enrollment and preflight,
namespace and batch rejection, stable identity, superseded-chunk removal, unrelated
document and checkpoint isolation, retry safety and canonical rollback on storage
failure. Connector scans, deletion, dry-run and CLI exposure remain out of scope.

### Phase 3: Implement the minimum durable-run lifecycle

This phase implements the subset of 0014 required for trustworthy background tasks.
It should not absorb context compaction or speculative working-state features.

#### 3A. Typed outcomes and compatibility mapping

Add the specified run outcome/reason representation and map legacy four-state behavior
compatibly. Expose it through store APIs, events, CLI JSON and human inspection.
Implement the selected completion/control mechanism with backward-compatible ordinary
completion.

**Slice status (2026-10-04): Implemented.** Additive run columns and deterministic
legacy backfill now preserve the four-value status projection while public store and
CLI inspection expose typed lifecycle, outcome, reason code and bounded detail.
Opt-in `run.blocked` and `run.request_user_input` controls are runtime-owned,
capability-free, collision-protected and validated without database, provider,
network or process access. Runtime and migration fixtures cover completion,
suspension, malformed/mixed controls, failure, turn/duration limits, cancellation,
legacy rows and existing recovery/delegation behavior. Phase 3C checkpoint/recovery
evolution remains pending.

#### 3B. Cumulative budgets

Persist and enforce approved model-turn, duration, token and tool-call budgets across
new and resumed execution. Preserve “unknown” when a provider does not report usage;
apply the specified fallback rather than treating missing usage as zero.

**Slice status (2026-10-04): Implemented.** Agent resources retain their compatible
turn and duration defaults and now accept optional positive cumulative token and
tool-call limits. Run rows expose configured budgets, elapsed time and typed usage;
model attempts, reported or missing token usage and started ordinary tool calls are
recorded atomically with their source events. Runtime preflight prevents partial tool
batches, token-budgeted runs fail closed on missing usage, and delegation debits each
active ancestor without resetting child or root limits. Existing resource versions
remain stable when the new optional limits are omitted, legacy rows preserve unknown
token accounting, and resume continues from the persisted counters. Phase 3C still
owns checkpoint-schema and recovery-disposition evolution.

#### 3C. Recovery and checkpoint evolution

Version checkpoints and migrations for typed outcomes, cumulative budgets and the
context-builder seam. Preserve workspace, resource, provider, binding, policy,
deadline and completed/uncertain-effect validation.

**Gate L complete:** task workers may now project authoritative run outcomes and use
the specified recovery categories.

**Acceptance:** tests distinguish completed, blocked, needs-input, failed,
limit-exceeded and cancelled as specified; budgets do not reset on resume; legacy
runs remain inspectable; uncertain effects are never replayed.

**Slice status (2026-10-05): Implemented.** Checkpoint schema v2 persists cumulative
budgets and usage, the original deadline, typed-outcome schema version, compatible
context-projection metadata and stable provider implementation identity. Version 1
snapshots retain their original binding and decoding path. Store and CLI inspection
now expose a deterministic persisted-state recovery disposition without resolving
resources, constructing providers, acquiring execution locks or invoking external
systems; live resume remains the authoritative validation boundary. Focused fixtures
cover compatible projection, v2 tamper rejection, disposition categories, cumulative
counter retention and uncertain-effect reconciliation.

### Phase 4: Deliver durable background tasks

#### 4A. Task persistence and idempotent submission

Add additive `agent_tasks` migrations and `AgentTaskStore`. Implement workspace-scoped
request keys, immutable digests, queue bounds, accepted identities, list/inspect and
queued cancellation. Submission performs no provider/tool call.

**Slice status (2026-10-05): Implemented.** The main crate now persists workspace-scoped
accepted tasks and gapless task events separately from run history. The public trusted-host
submission path resolves canonical agent, reachable-resource, model-provider, advertised-tool,
binding-authority and host-policy identity before database initialization; fixture counters
demonstrate that static inspection and submission do not construct a provider, bind or execute
a tool, resolve credentials, invoke a model, or make a network request. Atomic submission
enforces request-key idempotency, immutable request and accepted-identity digests and bounded
queue capacity. Inspection, bounded listing/event reads and idempotent queued cancellation are
available without adding claims, leases, workers, run linkage, reconciliation or CLI commands.

#### 4B. Worker claims and execution

Implement transactional conditional claims, leases, heartbeat, concurrency, attempts,
graceful shutdown and resource-drift checks. Link a claimed task to an ordinary run
before model work proceeds. Extend the public host API with bounded worker operation.

**Slice status (2026-10-05): Implemented.** `AgentTaskStore` now grants exclusive
database-clock claims with opaque tokens, bounded pre-run attempts and token-guarded
heartbeats and finalization. The additive trusted-host worker API validates all worker
bounds before initialization, runs only when explicitly awaited, limits active executions,
stops claiming on shutdown and keeps leases alive while ordinary runtime cancellation
finishes. Workers recompute the accepted identity before run creation; drift becomes a
terminal scheduling result without provider or tool construction. Root run creation,
lineage/event initialization, immutable task linkage and `task.run_linked` commit in one
SQLite transaction before providers are constructed, then the worker drives that run
through the same `AgentRuntime` execution path as direct runs. Focused contention,
stale-token, rollback, drift, sanitized setup-failure, concurrency, heartbeat and graceful
shutdown fixtures cover the boundary. Cancellation requests, expired-lease reconciliation,
run-state projection and CLI/service surfaces remain Phase 4C/4D work.

#### 4C. Cancellation and reconciliation

Project the linked run's authoritative outcome into task inspection. Reconcile expired
claims by distinguishing no run, terminal run, resumable/suspended run and uncertain
effect. Never start a second run solely because a lease expired.

**Slice status (2026-10-05): Implemented.** Task inspection now projects the linked
run's typed lifecycle, outcome, reason and recovery disposition without copying them
into scheduling state. Cancellation is workspace-scoped and idempotent across queued,
claimed, linked and terminal tasks; workers observe durable requests and let the normal
runtime record its stopped outcome before terminal task projection. Expired claims are
retaken with a fresh token and reconciled according to persisted run truth: pre-run work
is requeued, exhausted or cancelled; complete and suspended runs project `run_stopped`;
resume-eligible runs use authoritative same-run resume; and unsafe or incompatible state
becomes `reconciliation_required` or `restart_required`. A typed process lock conflict
keeps the reconciler observing and heartbeating without invoking the owned run, then
releases scheduling work for a later lease after the execution owner disappears. Focused
fixtures cover cancellation/link contention, stale tokens, every recovery-disposition
branch, provider-independent inspection, cancellation delivery and the invariant that
reconciliation never creates a second run. CLI and service-manager surfaces remain 4D.

#### 4D. CLI and operations

Add only spec-approved enqueue, worker, job list/inspect and cancel surfaces. Keep the
worker foreground-first; add service-manager-neutral runbook examples afterward.

**Acceptance:** submitter exit does not lose accepted work; two workers cannot own one
task; duplicates and conflicts are transactional; crash injection never duplicates an
uncertain effect; direct `ctx agent run` remains unchanged.

### Phase 5: Extend long-running run quality

These 0014 slices can proceed after the task layer because they refine execution
quality rather than submission ownership.

#### 5A. Structured working state

Add the selected materialized state representation and transaction/event relationship.
Start with deterministic runtime facts; add model-authored fields only through the
specified constrained mechanism.

#### 5B. Bounded context projection

Add measured recent/history selection and traceable compaction over the compatibility
builder. Preserve objective, newest observation, tool-call validity, active blockers,
validation and artifact references. Treat summaries as derived state with provenance.

#### 5C. Recoverable observations and loop safeguards

Implement the specified tool outcome taxonomy and allow only approved recoverable
errors back into the model loop. Bound identical-call/failure repetition without
weakening permission or uncertain-effect failures.

#### 5D. Suspended continuation

Support blocked and needs-input continuation only after state ownership and checkpoint
safety are settled. Decide whether continuation resumes the same run or creates a
linked run; do not introduce a Session abstraction without a separate requirement.

**Acceptance:** a 25-plus-turn fixture remains bounded and inspectable; compaction
retains required evidence; typed suspension and continuation preserve budgets and
recovery identity.

### Phase 6: Integrated hardening and handoff

#### 6A. Cross-track fixture application

Build a small, domain-neutral fixture host that:

- registers one scoped compiled tool;
- configures a local model;
- submits a durable background task;
- observes typed run/task outcomes;
- writes a fixture document in host code; and
- requests targeted refresh and verifies retrieval.

The fixture proves composition. It must not introduce wiki, hook, publication or
vendor-specific behavior into runtime code.

#### 6B. Failure matrix

Exercise provider down/model missing, queue full, duplicate/conflict, worker crash,
lease expiry, cancellation, blocked/needs-input, token/tool/time exhaustion, resource
drift, ingestion rejection, embedding failure and vector-sidecar failure. Verify that
each layer reports its own state without claiming downstream success.

#### 6C. Compatibility and performance

Run migration tests from pre-0014 databases, synchronous runtime regressions,
declarative binding tests, multi-workspace isolation, MCP/profile compatibility and
connector sync equivalence. Measure queue claim overhead, local cold/warm inference,
context projection size and targeted-versus-full refresh. Set thresholds from observed
baselines rather than inventing them in advance.

#### 6D. Documentation graduation

Update authoritative specs to match delivered behavior, add setup/operations
runbooks, mark implemented designs Reference, and move PRDs through their lifecycle
only after their individual success criteria are evaluated. Application authors then
consume the public contracts without importing their domain into Context Harness.

### Phase and slice matrix

| Slice | Primary design | Depends on | Can run in parallel with | Review boundary |
|---|---|---|---|---|
| 0A lifecycle contract | 0014 | PR 34 foundation | 0B, 0D | Docs only |
| 0B provider contract | 0015 | SPEC-0016 | 0A, 0D | Docs only |
| 0C task contract | 0016 | 0A | 0D | Docs only |
| 0D refresh contract | 0017 | current ingest specs | 0A, 0B | Docs only |
| 1A context seam | 0014 | 0A direction | 1B, 1C | Behavior-preserving code |
| 1B provider catalog | 0015 | 0B direction | 1A, 1C | Behavior-preserving code |
| 1C shared ingestor | 0017 | characterization tests | 1A, 1B | Behavior-preserving code |
| 1D host builder | 0016 | PR 34 public catalogs | 1A–1C | Direct-run public API |
| 2A Ollama | 0015 | 1B, Gate P | 2B, Phase 3 | One provider vertical |
| 2B targeted refresh | 0017 | 1C, Gate I | 2A, Phase 3 | One ingestion vertical |
| 3A–3C run lifecycle | 0014 | Gate L contract, 1A | Phase 2 | Run semantics only |
| 4A task store | 0016 | Gate Q, 1D | late Phase 3 tests | Submission only |
| 4B worker | 0016 | 4A, 3A–3C | Phase 5 | Claim/execution only |
| 4C reconciliation | 0016 | 4B, Gate L | Phase 5 | Recovery only |
| 4D task CLI | 0016 | 4A–4C | Phase 5 | CLI/runbook only |
| 5A–5D long-run quality | 0014 | Phase 3 | Phase 4 | One state/context concern each |
| 6 integrated hardening | all | delivered verticals | none | Integration and docs |

### Per-slice verification baseline

Every code slice should run, in addition to its targeted tests:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

Where an optional platform feature cannot run in the normal CI environment, add
fixture coverage in CI and record an explicit opt-in live command in the runbook.
Documentation-only decision slices validate relative links, indexes and diff
whitespace and state clearly that no behavior has shipped.

## Alternatives Considered

### One implementation epic and one large pull request

This minimizes temporary seams but combines providers, schemas, migrations, workers,
context state and ingestion into an unreviewable risk surface. Independent PR-sized
slices make invariants and regressions easier to evaluate.

### Finish all of DESIGN-0014 before other tracks

Background reconciliation needs typed lifecycle/recovery, but local generation and
targeted refresh do not. Context compaction and working-state quality are not queue
prerequisites. Waiting would serialize unrelated work and delay useful capabilities.

### Implement background tasks before lifecycle decisions

This would force the task layer to invent execution outcomes and later migrate them
to 0014, creating competing state machines. Specify and implement the minimum shared
lifecycle first.

### Implement the motivating application in the framework repository

This would make generic APIs follow one domain's hook, file and publication rules.
Use a neutral fixture here and keep application implementation in its own repository
or vault-driven project.

### Write all specs before introducing compatibility seams

Public behavior needs specs, but small behavior-preserving seams can validate module
boundaries and reveal constraints. Allow seams after directional review while keeping
new public behavior behind the authoritative contract gates.

## Acceptance Criteria

- Each PRD/design has an explicit contract gate, implementation slices and success
  criteria mapped into the phase matrix.
- Local provider and targeted refresh can ship without waiting for background tasks
  or context compaction.
- Background tasks consume one authoritative run-lifecycle vocabulary and do not
  duplicate blocked, needs-input, limit or recovery semantics.
- Every persistence change is additive, migration-tested and compatible with legacy
  inspection.
- Every new execution path uses existing policy, binding identity, history,
  cancellation and conservative recovery contracts.
- A neutral fixture composes local generation, background work, typed runs and
  targeted refresh entirely through public APIs.
- Full all-features Rust validation and existing compatibility suites pass before a
  phase is declared complete.
- No application-specific hook, content or publication behavior enters Context
  Harness core.

## Risks

- Parallel tracks may define incompatible host builders or identity types. Phase 0
  must name shared types and owners before public APIs stabilize.
- Compatibility seams can become permanent half-abstractions. Each seam needs a named
  consumer and deletion/extension criterion.
- The lifecycle subset may grow until Phase 4 is blocked on all of DESIGN-0014. Keep
  Gate L limited to typed outcomes, budgets, cancellation and recovery categories.
- Provider and ingestion partial failures can be flattened into generic task failure.
  Preserve layer-specific structured outcomes and references.
- Migration stacking across run, task and working-state tables can obscure failures.
  Test upgrades from representative historical schemas at every persistence slice.
- Live local-model quality may fail even when protocol tests pass. Keep quality
  evaluation separate from adapter correctness and never add cloud fallback silently.

## Open Questions

1. Which maintainers own Gates L, P, Q and I, and what review marks each resolved?
2. Should behavior-preserving seam PRs wait for full specs or only approved directional
   decisions recorded in the designs?
3. Does `AgentHostBuilder` land with provider catalog work or task-host work?
4. Which historical database versions form the required migration test matrix?
5. What neutral fixture application best exercises all public APIs without becoming a
   hidden domain feature?
6. Which phase first warrants a release, and should local generation and targeted
   refresh release independently before background tasks?
7. Which designs move from Draft to Planning once this sequence is approved?

Resolve the ownership and contract-gate questions before implementation begins. The
phase order may change as prototypes reveal constraints, but dependency direction and
the task/run authority boundary should remain explicit.
