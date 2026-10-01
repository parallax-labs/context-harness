# DESIGN-0013: Declarative Tool Bindings and Runtime Extensions

**Status:** Approved for implementation
**Date:** 2026-09-30
**Author:** Context Harness contributors
**Related:** [PRD-0013](../prd/0013-declarative-tool-bindings.md), [ADR-0025](../adr/0025-declarative-tool-bindings.md), [DESIGN-0010](0010-local-agent-runtime.md), [DESIGN-0011](0011-local-agent-runtime-execution-plan.md)

## Context

The initial runtime slices are implemented, but the broader declarative architecture
is incomplete. DESIGN-0010 section 4 separates capability implementation from resource
composition; section 13 calls for Rust/Lua/MCP tools through one registry. A complete
standalone binding syntax was never specified. This design fills that gap explicitly
rather than presenting new proposed syntax as an existing contract.

The wiki manager is deferred until this engineering is complete. Its detailed 0012
planning documents are withdrawn from this branch; Git retains the prior discussion.
Do not introduce wiki-specific tools, hook logic or write rules in this milestone.
Use small unrelated fixtures to prove generality, then revisit application design.

### Current implementation audit

| Area | Available in PR #33 | Remaining gap |
|---|---|---|
| Agents | Standalone TOML, named tool allowlists, permissions, models | Select fully resolved bindings rather than only fixed names/discovered MCP tools |
| Tool interface | Existing Tool and ToolRegistry | Descriptor/factory/binding contract and injectable runtime catalog |
| Local runtime | Fixed search/get, developer tools, agent.invoke | Generic validation and dispatch instead of tool-name conditionals |
| Lua/Rust extensions | Existing server integration | Runtime integration with enforceable authority/trust and context |
| MCP clients | Configured stdio, discovered namespaced tools, conservative approvals | Optional declared aliases/restrictions with schema/identity checks; no privilege downgrade |
| Policy | Host/agent capability intersection | Bound path/source authority and immutable configuration enforcement |
| Recovery | Checkpoints bind current tools/config | Resolved binding plus implementation identities and change detection |
| Tool resources | Legacy script configuration | Standalone schema, precedence, provenance and static diagnostics |

Evidence anchors: `agent_runtime/mod.rs` constructs the local registry and branches
on tool names for argument validation; `config.rs` defines legacy script configuration;
`tool_script.rs` exposes Lua adapters to the server; `agent_runtime/mcp.rs` discovers
external tools. Recheck these anchors against the checkout before implementation.

## Proposal

### Resolution and execution

```text
host implementation catalog + trusted host authority
                  + selected tool resources
                  -> validated resolved tool bindings
agent resource -> selected bindings -> existing ToolRegistry
                  -> generic validate / authorize / approve / execute / record
```

Keep one registry and invocation lifecycle. An implementation descriptor supplies a
stable implementation ID/version, configuration contract, argument/result contracts,
required capabilities and supported restrictions. A trusted factory resolves fixed
configuration and returns a bound Tool adapter. Distinguish generic configuration
validation from implementation-specific semantic checks; schemas alone do not enforce
filesystem or source authority.

A tool resource names its public tool, implementation/backend reference, description,
fixed configuration and requested restrictions. The public schema is derived from
or checked against the implementation contract. Do not allow a declaration to hide
required arguments, change incompatible types, or silently override implementation
validation. Initially support fixed parameters and explicit scoping, not arbitrary
expression evaluation, shell interpolation or a general workflow language.

The exact TOML schema, directory layout, descriptor/factory boundary, authority
rules, backend trust modes, CLI surface, and identity/recovery behavior are resolved
by [SPEC-0023](../spec/0023-declarative-tool-bindings.md).

### Example usage pattern: a release reviewer

A developer wants an agent to compare a proposed release summary with engineering
notes and indexed release decisions. This example demonstrates resource composition
without introducing a release-specific tool into the runtime.

**Illustrative future workflow:** the binding declarations below are expressed as a
table because their file layout and TOML schema are not decided. The agent TOML uses
existing agent-resource syntax, but its named bindings are not implemented today.
This is not a runnable configuration or a claim that the binding layer has shipped.

#### 1. The host grants a limited set of capabilities

The host integrator registers trusted file-reader and keyword-retrieval
implementations. The host grants read access to two fixture directories, `./release`
and `./engineering-notes`, and retrieval access to an indexed source with the
illustrative ID `filesystem:release-decisions`. Paths resolve against a configured
workspace root, never a model-selected working directory. The underlying retrieval
implementation supports enforcement of the source restriction.

This enrollment is host-owned. A project tool declaration cannot grant itself
access to another directory or source. No filesystem write, shell execution or
external process capability is granted.

#### 2. The author declares three named tool bindings

| Public tool name | Registered implementation | Fixed binding | Model-visible arguments |
|---|---|---|---|
| `release.read` | Generic scoped file reader | Root: `./release`; bounded UTF-8 output | Relative `path` within that root |
| `engineering.read` | The same generic scoped file reader | Root: `./engineering-notes`; bounded UTF-8 output | Relative `path` within that root |
| `decisions.search` | Generic keyword retrieval | Source: `filesystem:release-decisions`; result limit: 5 | `query` |

The actual resource files will encode these bindings using the schema chosen in
slice 1. The implementation supplies validation and supported restrictions; the
resource supplies names and fixed settings. Neither the root, source nor fixed
result limit is a model-overridable argument.

#### 3. The agent selects the bindings

An illustrative agent resource would be:

```toml
[agent]
name = "release-reviewer"
description = "Check release claims against approved project evidence"
model = "review-model"
tools = ["release.read", "engineering.read", "decisions.search"]

[agent.execution]
max_turns = 6
timeout_seconds = 120

[agent.permissions]
mode = "read-only"

[prompt]
system = """
Review the release summary against engineering notes and release decisions.
Cite the files or documents supporting each finding. Report unsupported claims
and missing evidence. Return a review; do not modify files.
"""
```

`review-model` refers to a separately configured, supported model alias; this
example does not prescribe a provider or imply that local Ollama support exists.
The model choice is independent of the tool-binding composition.

#### 4. Inspect, validate, then invoke

Before execution, the author inspects the effective bindings and checks their
implementation identities, public schemas, roots/source restrictions, versions and
resource origins. Static validation rejects unknown implementations, unsupported
restrictions and name collisions without executing tools or calling a model.
Exact tool inspection commands will be set in the spec.

After the binding layer is implemented, the existing agent invocation shape can
run this composition:

```sh
ctx agent run release-reviewer "Review summary.md for unsupported release claims"
```

The agent might call `release.read` with `{"path":"summary.md"}` and
`decisions.search` with `{"query":"supported deployment targets"}`. The runtime
resolves the selected adapters, validates the effective calls, enforces policy,
executes through ToolRegistry, and records the public binding and implementation
identities. It returns a review with evidence references and a durable run ID.

An attempt to supply `root`, override the source, read `../private.txt`, or access
a symlink outside an enrolled root fails before the requested access occurs.
The same reader implementation serves both public read tools without runtime
branches for `release.read` or `engineering.read`.

#### 5. Reuse the capability for another application

A second fixture can bind the same reader to a test-results directory and select
it from a different agent resource. This requires resource changes, not changes to
the execution loop. If an application needs a genuinely new operation, an extension
author implements and registers that capability once; resource authors then bind
and reuse it through the same contracts. Declarative composition does not generate
arbitrary executable behavior.

The future wiki manager should follow this pattern after the foundation is verified.
Its domain behavior and additional integration requirements remain deferred.

### Resource resolution and identity

Reuse agent-resource configuration provenance: explicit/environment/pinned config
isolation, deterministic global/workspace discovery, explicit whole-resource overrides,
and clear duplicate-name failures. Specify reserved built-in/delegation/MCP names and
migration behavior before implementing aliases. Separate the implementation catalog
from resource discovery: a resource cannot dynamically link arbitrary Rust code or
implicitly install/download an extension.

Resolve references before model invocation; report missing implementations and illegal
bindings without side effects. Discovery and inspection do not execute Lua modules,
spawn MCP processes or query a model. Where metadata requires executing an extension,
report it as unresolved until an explicitly authorized preparation step; never claim
full offline verification of remote metadata.

An effective binding identity includes resource content, public schema, fixed config,
restrictions and implementation identity/version. Keep secret references as references,
resolve values only at execution, and redact diagnostics. Do not persist raw secrets
or use their values as public identity material. Define provider/config change rules
for secret-reference changes separately from rotating a secret's value.

### Authority and parameter binding

Effective authority is bounded by host grants, implementation capabilities, binding
restrictions and agent declarations. Fixed roots, source IDs and injected context are
not model-overridable parameters. Validate incoming arguments, compose fixed/bound
values without ambiguous merge precedence, then validate effective arguments and
semantic restrictions before any side effect. Reject an override attempt rather than
silently accepting a misleading model request.

Implementations explicitly advertise which scopes they can enforce. A reader can
confine paths using a host-issued scoped accessor; a retrieval tool can enforce source
filters including direct document lookup. A process with ambient filesystem access
cannot become directory-scoped merely because TOML says so. Unsupported scope claims
fail closed. Path normalization, symlink escapes, source-qualified IDs, output bounds
and approval arguments need adversarial tests.

Host-injected approvals cover the exact effective invocation. Preserve public binding
name, implementation identity and sanitized effective restrictions in history so an
alias cannot hide what was authorized. Extend policy without changing legacy clients'
behavior implicitly. Do not derive required capabilities from resource assertions alone.

### Backend integration

- **Built-in/Rust:** replace manual construction with trusted registered factories.
  Existing public names resolve to compatibility bindings. Rust extension authors
  register through a documented library API; runtime configuration selects registered
  code rather than loading arbitrary libraries. Remove name-based argument-validation
  fallthrough. Runtime-aware delegation may need explicit injected context, not a
  domain-name check masquerading as a generic extension API.
- **Lua:** adapt existing scripts through the same lifecycle, but audit exposed host
  APIs and module initialization. Restrict callable host functions using host-owned
  authority where enforceable; otherwise classify the script as trusted privileged
  code requiring explicit policy, or reject that runtime mode. No automatic promotion
  of server scripts into unattended read-only runtime tools.
- **MCP:** preserve startup and invocation approvals, bounded discovery, and conservative
  process/external-side-effect capabilities. A declared alias can constrain arguments
  but cannot constrain the server's ambient behavior. Validate the discovered schema
  against the binding after authorized startup and reject drift. Keep external-session
  resume unsupported until separately designed.

### Inspection, compatibility and recovery

Extend existing tool list/show/validate surfaces where possible; inspect current CLI
contracts before choosing new flags. Display origin, scope, implementation identity,
public schema, fixed settings with secrets redacted, effective restrictions and trust
requirements. Clearly distinguish static validation from authorized execution testing.

Record resolved binding identity in run/checkpoint metadata. Resume rejects changes
that affect implementation behavior, schemas, authority or bound context. Preserve
existing unknown-capability denial, uncertain-side-effect reconciliation and external
session/tree restrictions. History reads should not require loading executable code.
Legacy inline/script configuration needs adapters or an explicit migration path;
never silently reinterpret previously accepted names or merge precedence.

## Implementation Plan

This PR adds documents only for this milestone. First review this design, resolve
open choices, accept/amend ADR-0025 and create an authoritative spec. The user tests
and merges the existing runtime foundation. Subsequent implementation follows the
agreed spec; do not begin the slices below in this documentation update.

| Slice | Deliverable | Requirement / acceptance gate | Status |
|---|---|---|---|
| 1. Contracts | Descriptor, factory, bound-tool and authority contracts; chosen resource schema | D1/D3/D4: distinguish implemented capabilities from declarations; reject unenforceable scopes | Complete |
| 2. Resolution | Resource parser, layered discovery, aliases, provenance and identities | D1/D2/D6: deterministic results, explicit isolation, collision/override tests, side-effect-free inspection | In progress |
| 3. Runtime integration | Host-supplied catalog and generic adapter validation/dispatch; compatibility bindings | D3/D5/D8: second fixture tool requires no name branch; built-ins retain argument/policy behavior | In progress |
| 4. Extension backends | Rust API, Lua authority adapter and MCP alias validation | D4/D5/D8: approved extensions use common lifecycle; unknown authority denied; remote schema drift detected | In progress |
| 5. History/recovery | Binding metadata, checkpoint compatibility and legacy migration | D6/D7/D8: redacted inspection; changed implementation/config cannot silently resume | In progress |
| 6. Acceptance and docs | Two unrelated fixture compositions, adversarial tests, spec and runbook validation | D1–D8: complete foundation demonstrated; limitations explicit | Pending |

Dependencies: 1 precedes 2/3; 4 follows their integration; 5 follows stable identities
and adapters; 6 verifies all. Supporting input/schema libraries are selected only after
reviewing existing dependencies. Use focused tests per slice, then the repository's
feature/build matrix at integration gates. Do not label completion from one happy-path
fixture or a fake provider response alone.

### Acceptance matrix

1. Bind one reader twice to different fixture roots and expose both names to an agent;
   verify argument overrides, traversal and symlink escapes cannot cross the scopes.
2. Bind retrieval to a distinct source and test both search and direct lookup. This
   second use case contains no wiki behavior and needs no runtime name dispatch.
3. Add an approved Rust extension and Lua fixture; exercise invalid arguments, denied
   privileges, approval, execution failure, cancellation and output limits. Prove Lua
   module loading cannot bypass the authorized preparation boundary.
4. Bind a local MCP fixture alias; verify startup approval, discovered schema mismatch,
   argument constraints and no downgrade of external process privilege.
5. Exercise duplicates, reserved names, explicit-config isolation, workspace overrides,
   missing implementations and secret-safe diagnostics without starting extensions.
6. Change resource/config/schema/implementation identity between checkpoint and resume;
   verify rejection for incompatible bindings and unchanged history access.
7. Run existing CLI, agent, server Lua/Rust, MCP prompt and workspace regressions.

## Alternatives Considered

- Hardcode a wiki tool suite now: faster demonstration, but conceals the missing
  extension contracts. Defer the application until the foundation is proven.
- Treat a list of existing names as complete declaration: supports selection but not
  composition, configuration binding, authority or reproducible implementation identity.
- Introduce a second runtime-only tool system: duplicates existing lifecycle and
  compatibility responsibilities; evolve Tool/ToolRegistry through adapters instead.
- Trust declarations as capability attestations: does not constrain arbitrary code;
  require trusted descriptors and enforceable host boundaries.
- Build a general workflow/configuration language: unnecessary for fixed tool binding;
  defer arbitrary transformations and expressions.

## Resolved Questions

The six decision groups below are resolved normatively by SPEC-0023:

1. Exact resource schema/version, discovery directories, reserved namespaces and
   compatibility/override rules for existing script configuration and MCP names.
2. Descriptor/factory API and implementation version guarantees, including Rust builds,
   Lua module dependencies and externally discovered MCP schemas.
3. Authority representation for paths/sources and host-injected runtime context;
   which restrictions each backend can actually enforce, and how unsupported ones fail.
4. Fixed-argument conflict rules, schema dialect/validator, configuration secrets and
   whether aliases may narrow descriptions/results without misleading clients.
5. Lua initialization/host API audit and trust modes; behavior of existing server-only
   extensions that cannot safely run under narrower runtime authority.
6. Checkpoint identity/migration policy and CLI diagnostics for unresolved remote metadata.

## Handoff and completion gates

- Document chain: PRD-0013 -> DESIGN-0013 -> proposed ADR-0025 -> future spec -> tested
  implementation -> verified setup/operation runbook. Index every artifact when created.
- This update removes detailed wiki planning, changes no runtime behavior and does not
  activate hooks, download models or grant unattended writes.
- Current source anchors and missing functionality are in the audit above; completed
  initial slices remain recorded in DESIGN-0011 with a scope qualification.
- Next action: implement slice 1 against SPEC-0023, then proceed through the remaining
  slices only after each focused acceptance gate passes.
- After the foundation passes, revisit the wiki manager as the first real application.
  Its later design will still need local inference, executable MCP exposure, hook
  delivery, publication/reindexing and budget measurement; this milestone does not
  quietly claim those separate capabilities are solved.
