# SPEC-0023: Declarative Tool Bindings

**Status:** Authoritative  
**Date:** 2026-09-30  
**Scope:** Standalone tool resources, trusted implementation catalogs, authority,
runtime binding, inspection, extension backends, and recovery compatibility.

## Overview

Context Harness SHALL separate executable tool implementations from declarative
tool bindings. A trusted host registers implementations and grants authority. A
standalone resource assigns a public name, supplies fixed configuration, narrows
authority, and optionally fixes model-call arguments. Agent resources continue to
select public names. Declarations SHALL narrow host authority and SHALL NOT create
capabilities, load arbitrary native code, or turn configuration into a workflow
language.

Every selected binding SHALL execute through the existing `ToolRegistry`, runtime
policy, approval, cancellation, history, and output-bound lifecycle. The runtime
SHALL validate and dispatch a bound adapter without branching on its public name.
Existing public built-ins and fully qualified MCP tools SHALL remain available as
compatibility bindings.

## Definitions

- **Implementation:** Trusted executable behavior registered by the host.
- **Descriptor:** Side-effect-free metadata for an implementation: identity,
  version, schemas, capabilities, supported restrictions, and trust class.
- **Factory:** Trusted code that validates configuration and creates a bound adapter.
- **Binding:** The resolved combination of one tool resource, one implementation,
  host authority, fixed arguments, public schema, and effective restrictions.
- **Compatibility binding:** A host-created binding for an existing built-in,
  developer, delegation, or fully qualified MCP public name.
- **Preparation:** An explicitly authorized operation that may execute extension
  initialization or start an MCP process to obtain runtime metadata.
- **Static validation:** Resolution that reads configuration and descriptors but
  does not execute scripts, spawn processes, access the network, call models, or
  resolve secret values.

## Resource schema

Standalone tool resources SHALL be UTF-8 TOML files ending in `.toml` and SHALL
use `schema_version = 1` with the following shape:

```toml
schema_version = 1

[tool]
name = "release.read"
implementation = "builtin.scoped_file_read"
description = "Read a file from the release fixture"
override = false

[config]
root = "release"

[fixed]
max_bytes = 65536

[restrictions]
paths = ["release"]
max_output_bytes = 65536
```

`schema_version`, `tool`, `config`, `fixed`, and `restrictions` are the only
top-level keys. `schema_version` and `[tool]` are required. The other tables
default to empty. Unknown fields in `[tool]` and `[restrictions]` SHALL fail.
`[config]` and `[fixed]` accept only values allowed by the selected descriptor.

`tool.name` and `tool.implementation` SHALL contain 1–128 ASCII letters, digits,
underscores, hyphens, or periods. Names SHALL start with an ASCII letter or digit.
`tool.description`, when present, SHALL contain 1–1024 bytes after trimming.
`tool.override` defaults to `false` and replaces a lower-precedence binding as a
whole; tables SHALL NOT merge between resources.

The initial restriction vocabulary is:

| Key | Type | Meaning |
|---|---|---|
| `paths` | array of strings | Workspace-relative roots the implementation may access |
| `sources` | array of strings | Exact indexed source IDs the implementation may access |
| `max_output_bytes` | integer | Serialized successful result limit, 1 through 1,048,576 |

Paths SHALL be nonempty relative paths without a root, parent component, or NUL.
They SHALL be resolved against the canonical workspace root. Source IDs SHALL be
nonempty, at most 1024 bytes, and contain no NUL. Arrays SHALL contain no duplicate
normalized values. A descriptor SHALL reject every restriction it cannot enforce.
Resources SHALL NOT declare capabilities or trust classes.

`[fixed]` binds top-level model-call properties. A fixed property SHALL exist in
the implementation input schema. The resolved public schema SHALL omit that
property and remove it from `required`. An incoming call containing a fixed key
SHALL fail, even when its value equals the configured value. The runtime SHALL
then form the effective object as the disjoint union of validated public arguments
and fixed values. Recursive merge, interpolation, templates, expressions, and
model-selected configuration are not supported.

The implementation descriptor SHALL validate `[config]`, `[fixed]`, the derived
public schema, and the effective argument object. JSON input schemas SHALL use
Draft 2020-12 object schemas. A binding SHALL NOT add properties, weaken types or
constraints, or hide implementation-required model input except by fixing a valid
value. Descriptions may be replaced for composition, but inspection SHALL show
both the public and implementation descriptions.

## Secret references

New resources SHALL represent secrets only as an inline table containing exactly
`env`, for example `token = { env = "ISSUE_TOKEN" }`. The environment name SHALL
match `[A-Za-z_][A-Za-z0-9_]*`. Secret references MAY appear only where a descriptor
explicitly permits a secret. Static validation SHALL retain the reference and SHALL
NOT read the environment. The factory SHALL resolve it immediately before an
authorized preparation or invocation that needs it.

Inspection, errors, histories, binding identities, and checkpoints SHALL contain
the environment name and the marker `secret_ref`, never the value. Changing the
reference changes binding identity; rotating the referenced value does not. The
legacy `${VAR}` expansion contract remains unchanged for legacy configuration and
SHALL NOT be applied to standalone tool resources.

## Discovery, precedence, and reserved names

Tool resources SHALL use the same isolation and layering rules as standalone agent
resources:

1. Without explicit configuration, load sorted `.toml` files from
   `$XDG_CONFIG_HOME/ctx/tools`, then `<workspace>/.ctx/tools`.
2. With `--config` or `CTX_CONFIG`, load only the `tools` directory beside that
   selected config file and mark its scope `explicit`.
3. A missing directory is empty and SHALL NOT be created by inspection.
4. Duplicate names within one layer SHALL fail. A higher layer collision SHALL
   fail unless the higher resource sets `tool.override = true`.
5. Parsing and static validation SHALL be deterministic and side-effect-free.

The public names `search`, `get`, `workspace.*`, `git.*`, `process.exec`, and
`agent.invoke` are reserved compatibility names and SHALL NOT be declared by a
resource. The `runtime.*` namespace is internal and SHALL NOT be public. The
`mcp.*` namespace is reserved for fully qualified discovered MCP names and SHALL
NOT be used as an alias. A resource name SHALL NOT collide with a legacy configured
Lua/Rust tool or a compatibility binding.

Implementation IDs use host-owned namespaces. Core supplies `builtin.*`; a host
may register `rust.*`; configured Lua adapters use `lua.*`; MCP references use the
existing `mcp.<server>.<remote-tool>` form. A resource SHALL NOT install, download,
dynamically link, or register an implementation.

## Implementation catalog and factories

The library SHALL expose a host-constructed implementation catalog. Registration
SHALL reject duplicate implementation IDs. Each immutable descriptor SHALL provide:

- implementation ID and version;
- implementation and default public descriptions;
- configuration and input schemas;
- required runtime capabilities;
- supported restrictions and whether each is enforceable;
- trust class: `builtin`, `compiled`, `lua_privileged`, or `mcp_external`;
- a factory that returns a bound `Tool` adapter plus generic validators.

Implementation versions SHALL be nonempty stable strings of at most 256 bytes.
Core built-ins SHALL combine the crate version with an implementation contract
version. Compiled host extensions SHALL supply a version and SHALL change it when
behavior, schema, capability needs, or authority enforcement changes. A Lua version
SHALL include the SHA-256 digest of its script bytes and its side-effect-free host
manifest. An MCP version SHALL include the selected server configuration digest,
remote name, and the authorized discovery schema digest.

`AgentRuntime::new` SHALL preserve the current compatibility catalog. A trusted
embedding host SHALL be able to construct a runtime with an explicit catalog and
host authority. Project resources and model input SHALL NOT mutate either after
runtime construction. A factory SHALL receive resolved nonsecret configuration,
secret resolvers, canonical host-issued accessors, and effective restrictions; it
SHALL NOT infer broader access from the process environment or current directory.

The bound adapter SHALL implement the existing `Tool` trait. Generic runtime code
SHALL obtain public schema, capabilities, binding metadata, public-argument
validation, effective-argument validation, and execution from the adapter. Adding
a second binding or a registered implementation SHALL require no public-name branch
in the execution loop.

## Authority and invocation

Effective authority SHALL be the intersection of host grants, implementation
requirements, binding restrictions, agent permissions, and runtime policy. The
host SHALL issue canonical path/source accessors; raw resource strings are not
authority. A binding SHALL fail before model invocation if a required grant is
missing or a restriction cannot be enforced.

Path accessors SHALL repeat containment and symlink checks at execution. A scoped
reader SHALL reject absolute paths, parent traversal, special files, and symlink
escapes. Source accessors SHALL apply the exact allowed source set to search and
direct lookup; a returned document outside that set SHALL be rejected. Output
bounds SHALL apply to serialized successful results before persistence or model
continuation.

Authorization and approval SHALL cover the public binding name, implementation
identity, required capabilities, sanitized effective restrictions, and the exact
effective arguments. Fixed arguments and host-injected context SHALL be visible in
the approval record unless they are secret values. Declaration validation,
authorization, argument validation, and effective-argument construction SHALL
complete before tool start or any side effect.

## Backend rules

### Built-in and compiled Rust

Core SHALL express existing local tools as compatibility bindings backed by trusted
descriptors. Existing schemas, capabilities, validation, and public behavior SHALL
remain unchanged. Compiled extensions SHALL be registered explicitly by the host;
resources SHALL NOT name crates, library paths, or package URLs. Delegation MAY use
an injected runtime context supplied by its descriptor and SHALL NOT be recognized
by a generic public-name check.

### Lua

Standalone resources MAY reference only Lua implementations already present in
trusted host configuration. Version 1 supports only `lua_privileged`: a resource
cannot claim a Lua script is sandboxed or read-only. The host-owned Lua manifest
SHALL state the script path, version digest, configuration schema, input schema,
capabilities, permitted host APIs, and output limit without evaluating the module.

Static list/show/validate SHALL read the manifest and script bytes but SHALL NOT
evaluate Lua. Before the first module evaluation in a run, the runtime SHALL create
and authorize a synthetic `runtime.lua.prepare.<implementation>` invocation. Its
capabilities SHALL include every capability granted by the manifest's host APIs.
Non-interactive denial SHALL prevent evaluation. Module evaluation, argument
handling, execution, timeout, cancellation, output bounds, and history SHALL use
the common lifecycle. Existing server-only Lua tools remain server-only until a
host manifest explicitly enrolls them; legacy configuration is not auto-promoted.

### MCP

A resource MAY bind an alias to `mcp.<server>.<remote-tool>`. The alias retains
the server startup and per-call `process_execute` plus `external_side_effect`
requirements from SPEC-0020. Fixed arguments and output limits constrain only
requests the client sends; they SHALL NOT be described as sandboxing the server.
Path and source restrictions are unenforceable for MCP and SHALL be rejected.

Static validation SHALL report the binding as `remote_metadata = "unresolved"`
without starting the server. After separately authorized startup, discovery SHALL
require the remote schema to be an object schema, derive the public schema, validate
fixed values, and compute the effective version. Drift SHALL fail preparation before
the alias is advertised to the model. MCP-backed runs remain non-resumable under
SPEC-0020.

## Identity, history, and recovery

A resolved local binding version SHALL be `sha256:` plus the SHA-256 digest of
canonical deterministic JSON containing:

- parsed default-expanded resource content;
- implementation ID and version;
- implementation input schema and derived public schema;
- normalized fixed configuration and restrictions;
- secret references, but not secret values.

An MCP binding adds the authorized discovery schema and server configuration digest.
Formatting, comments, source paths, and secret rotation SHALL NOT change identity.
Resource provenance remains separate and SHALL include absolute path and scope.

Every invocation row or associated immutable binding snapshot SHALL retain the
public name, binding version, implementation ID/version, public schema digest, and
sanitized effective restrictions. History reads SHALL use stored snapshots and
SHALL NOT load implementations, evaluate Lua, start MCP, query a model, or resolve
secrets.

Checkpoints SHALL include the ordered selected binding versions and the catalog
contract version. Resume SHALL reject missing or changed bindings, implementation
versions, schemas, fixed configuration, restrictions, secret references, or injected
context identity before reopening the run. Secret value rotation alone SHALL NOT
invalidate a checkpoint. Existing uncertain-side-effect and external-session rules
remain unchanged.

## CLI contract

Existing `ctx tool init`, `ctx tool test`, and `ctx tool list` retain their legacy
Lua/server behavior. Standalone bindings use a new nested surface:

```text
ctx tool bindings list [--json]
ctx tool bindings show <name> [--json]
ctx tool bindings validate [<name>] [--json]
```

`list` SHALL return public name, description, implementation ID/version when known,
scope, binding version or unresolved state, and trust class in name order. `show`
SHALL additionally return provenance, implementation/public descriptions, public
schema, fixed keys and values with secrets redacted, effective restrictions,
capabilities, and preparation requirements. `validate` without a name SHALL validate
the complete static catalog; with a name it SHALL validate only that binding and
the compatibility collision set.

These commands are static. They SHALL NOT create a database, resolve secret values,
evaluate Lua, spawn MCP, make network calls, or call a model. Success for an MCP
alias means its local declaration is valid and its remote metadata is unresolved;
it SHALL NOT claim runtime schema validation. Execution testing remains `ctx agent
run` so policy and durable history are never bypassed.

## Compatibility and migration

Existing agent resources containing built-in, developer, delegation, or fully
qualified MCP names SHALL resolve to compatibility bindings with unchanged behavior.
Existing `[tools.script.*]`, server tool discovery, and Lua test commands SHALL retain
their contracts. They SHALL NOT become executable local-agent bindings implicitly.

A legacy Lua name or compatibility name colliding with a standalone binding SHALL
produce an actionable error; precedence SHALL NOT silently reinterpret it. The first
release SHALL NOT rewrite configuration or move resources automatically. Multi-
workspace MCP serving remains governed by SPEC-0014 and SHALL NOT discover workspace
runtime bindings unless that specification is explicitly extended.

## Acceptance criteria

1. One scoped reader implementation is bound twice to different fixture roots.
   Both work, while fixed-key overrides, absolute paths, traversal, and symlink
   escapes fail before access.
2. A retrieval implementation is bound to an exact source for search and direct
   lookup; cross-source results and IDs fail without a public-name branch.
3. Duplicates, unauthorized overrides, reserved names, missing implementations,
   invalid schemas, unsupported restrictions, and forged capabilities fail during
   static validation without database creation or extension startup.
4. Explicit configuration sees only its sibling `tools` directory. Global and
   workspace layering is deterministic and reports path, scope, and identity.
5. A registered Rust fixture and privileged Lua fixture exercise approval, failure,
   cancellation, output bounds, and durable metadata. Lua cannot evaluate before
   preparation authorization.
6. An MCP alias exercises startup authorization, schema drift, fixed arguments,
   per-call approval, and conservative capability reporting without enabling resume.
7. Inspection and history never reveal secret values or execute implementations.
   Binding/config/schema/implementation changes reject resume; secret rotation does not.
8. Existing CLI, agent, Lua/Rust server extension, MCP client/server, workspace,
   and recovery tests remain green.
