# SPEC-0015: Standalone Agent Resources Spec

**Status:** Authoritative  
**Date:** 2026-09-25  
**Related:** [DESIGN-0010](../design/0010-local-agent-runtime.md), [execution plan](../design/0011-local-agent-runtime-execution-plan.md), [MCP agents](0011-mcp-agents.md), [config resolution](0013-config-resolution.md)

## Scope

This spec defines standalone static agent resources and their discovery,
validation, and CLI inspection. Model calls are defined by SPEC-0016 and local
execution by [SPEC-0017](0017-local-agent-execution.md). Dynamic initial context,
named policy files, delegation and resume remain later implementation slices.

Existing inline TOML, Lua, and Rust prompt agents SHALL retain their existing
interfaces. Standalone files SHALL NOT be automatically registered as MCP prompts
in this slice. Library callers MAY adapt a resource with `prompt_agent()` using
the existing `Agent` trait.

## Resource format

A resource SHALL be a UTF-8 TOML file containing `[agent]` and `[prompt]`:

```toml
[agent]
name = "researcher"
description = "Answers questions using project context"
model = "reasoning"
tools = ["search", "get"]

[agent.execution]
max_turns = 12
timeout_seconds = 300

[agent.permissions]
mode = "read-only"

[prompt]
system = """
Search project context before answering. Cite the documents you use.
"""
```

- `name` and `model` SHALL be nonempty identifiers containing only ASCII letters,
  digits, underscores, hyphens and dots. `name` is the discovery key; filenames
  SHOULD match it but do not define the key.
- `description` defaults to an empty string. `tools` defaults to an empty list.
  Tool names SHALL be unique identifiers with the same character rules.
- `prompt.system` SHALL contain non-whitespace text.
- Execution defaults SHALL be 12 model turns and 300 seconds. Both values SHALL
  be positive integers; `max_turns` is a 32-bit value and timeout is a 64-bit value.
- Unknown fields or tables in the resource SHALL be rejected, including
  currently unsupported context/delegation declarations.
- `[agent] override = true` SHALL explicitly authorize replacing an earlier
  standalone resource with the same name. It defaults to false.

## Model references

Model aliases SHALL be declared in the effective Context Harness config:

```toml
[models.reasoning]
provider = "openai"
model = "your-provider-model-id"
api_key_env = "OPENAI_API_KEY"
```

`provider` and `model` SHALL contain non-whitespace text. `api_key_env` is optional
and SHALL be an environment variable identifier: first character an ASCII letter
or underscore, followed by ASCII letters, digits or underscores. Unknown model
fields SHALL be rejected. Inline API keys are not a supported field.

Effective resources SHALL reference configured model aliases. Model reference
checks SHALL run after resource overrides have selected the effective resource.
An overridden global resource MAY reference an alias absent from the effective
config. Its TOML structure must still be valid.

Loading and validation SHALL NOT read credential values, contact providers, or
open the context database for standalone/static agents. Provider support and tool
availability SHALL be checked by later runtime binding; successful declaration
validation does not assert either. `agent validate` SHALL also check unused model
alias declarations for valid names and fields.

## Permission declarations

The recognized capabilities SHALL be `read_only`, `workspace_write`,
`process_execute`, `network`, and `external_side_effect`.

Omitting permissions SHALL mean an allowed capability set of `["read_only"]` and
no approval capabilities. The only named `mode` currently supported SHALL be
`"read-only"`. Named mode SHALL NOT be combined with `allow` or a nonempty
`require_approval` list.

Explicit policy lists are also supported:

```toml
[agent.permissions]
allow = ["read_only"]
require_approval = ["workspace_write", "process_execute"]
```

An explicit `allow` list SHALL replace the default; `allow = []` allows nothing
without approval. Capabilities absent from both lists are denied by the intended
runtime policy. Duplicate capabilities and overlap between the two lists SHALL
be rejected. This slice parses these declarations; it does not execute tools or
enforce a sandbox. External MCP clients continue to own their own permissions.

## Discovery and precedence

Normal CLI discovery SHALL scan these directories in order:

1. `<ctx global config directory>/agents/*.toml`, using the existing
   `CTX_CONFIG_DIR` / XDG / home directory resolution.
2. `<current workspace root>/.ctx/agents/*.toml`.

The current CLI workspace root is the working directory, consistent with existing
config discovery; ancestor walking is not added. Missing directories SHALL be
ignored. Other read errors and malformed TOML SHALL fail with a resource path.
Discovery SHALL be nonrecursive and process filenames in sorted order. Files with
other extensions SHALL be ignored. The same canonical directory SHALL be read
only once if global and workspace paths coincide.

Workspace resources SHALL replace global resources only with explicit
`agent.override = true`. Replacement SHALL use the whole resource, never merge
individual prompt, tool, limit or permission fields. Duplicate names within one
directory SHALL fail even with `override = true`. The override marker MAY remain
on a resource when its global counterpart is absent.

Standalone names colliding with configured inline or Lua agents SHALL fail
catalog validation, even with `override = true`; users must rename or migrate the
legacy entry explicitly. Duplicate inline/Lua names SHALL be reported as
ambiguous by combined catalog commands.

With `--config`, `CTX_CONFIG`, or a pinned config, discovery SHALL instead use only
`<selected config parent>/agents/*.toml`. Ambient global and working-directory
resource directories SHALL NOT be imported. For example, an explicit
`.ctx/config.toml` uses `.ctx/agents`, while `custom/settings.toml` uses
`custom/agents`. Relative selected config paths resolve against the supplied root.
The library path resolver SHALL accept an explicit root for future registered
workspace runtime consumers; it SHALL NOT infer workspace roots from DB paths.

Resource discovery SHALL occur only in agent commands or explicit library calls,
so an invalid standalone file SHALL NOT break unrelated ingestion/retrieval/MCP
commands.

## Provenance and versioning

A loaded resource SHALL retain its absolute source path, scope (`global`,
`workspace`, or `explicit`) and version. The version SHALL be `sha256:` followed
by the SHA-256 hex digest of deterministic JSON serialization of the parsed,
default-expanded definition. Comments, TOML whitespace and source path SHALL
NOT affect the version. Declaration changes SHALL affect it.

The resource hash covers the model alias, not the resolved provider config or
credentials. Later runtime snapshots must separately record resolved model and
policy state for safe resume.

## CLI contract

- `ctx agent list [--json]` SHALL list standalone resources and configured inline
  and Lua agents in name order. Human output includes name, source, description
  and tools. JSON output is an array of catalog entries.
- `ctx agent show <name> [--json]` SHALL show the selected catalog entry. Resource
  entries include path, scope, version, model alias, limits, permissions and
  static system prompt. Unknown names fail with a nonzero exit status.
- `ctx agent validate` SHALL validate the complete catalog and model declarations,
  print agent/resource counts on success, and exit nonzero on failure.
- `ctx agent test <name>` SHALL preview standalone or inline static prompts.
  Static agents reject `--arg`. Existing Lua `test` behavior SHALL be retained,
  including arguments and dynamic prompt resolution; unrelated broken definitions
  SHALL NOT prevent testing a named legacy agent.
- `ctx agent init <name>` SHALL retain its Lua scaffolding behavior.

Catalog JSON entries contain `name`, `description`, `source`, `tools`, and
`arguments`. Static entries also contain `system_prompt`. Standalone entries
add `resource` containing `path`, `scope`, `version` and `definition`. The source
values are `resource`, `toml`, and `lua`. Omitted permission `allow` in JSON means
the read-only default described above.

Lua catalog metadata loading continues to evaluate configured Lua modules using
the existing loader; `show` and `validate` do not invoke their `resolve` function.
Built-in/custom Rust extension registration and existing MCP prompt discovery
remain governed by SPEC-0010 and SPEC-0011.

## Acceptance

`tests/agent_resources.rs` covers strict schemas, effective defaults, content
versions, duplicate/legacy collisions, full replacement, config isolation,
source provenance, CLI inspection, legacy targeted testing and `Agent` trait
adaptation. Existing workspace and Rust/MCP extension tests SHALL continue to pass.
