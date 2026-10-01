+++
title = "Build Local Agents"
description = "Configure durable standalone agents with scoped tools, approvals, history, and recovery."
weight = 2
+++

Context Harness can run standalone agents directly in a project. These agents
use a configured model, select a small set of local tools, and record their runs
in the project's SQLite database. They are different from MCP **profiles**: a
standalone agent owns a bounded model/tool loop and can be inspected or resumed
locally, while a profile only prepares a conversation owned by another client.
See [Profiles and Agents](@/docs/guides/agents.md) for a direct comparison.

This guide builds a generic project researcher. The same structure works for
code review, release preparation, incident analysis, migration planning, and
other workflows without assuming that your context lives in a wiki.

### What you will create

```
.ctx/
├── config.toml
├── agents/
│   └── project-researcher.toml
└── tools/
    ├── project-search.toml
    └── project-get.toml
```

The model sees `project.search` and `project.get`. Both are declarative aliases
over trusted built-in implementations and are restricted to one connector source.
The declarations narrow authority; they cannot load executable code or grant new
capabilities.

### 1. Configure the model and source

Create `.ctx/config.toml`:

```toml
[db]
path = ".ctx/data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "disabled"

[retrieval]
final_limit = 12
hybrid_alpha = 0.6
candidate_k_keyword = 80
candidate_k_vector = 80
group_by = "document"
doc_agg = "max"
max_chunks_per_doc = 3

[server]
bind = "127.0.0.1:7331"

[models.default]
provider = "openai"
model = "gpt-5-mini" # replace with a model available to your project
api_key_env = "OPENAI_API_KEY"

[connectors.filesystem.project]
root = "."
include_globs = ["**/*.md", "**/*.txt", "**/*.rs", "**/*.py", "**/*.ts", "**/*.tsx"]
exclude_globs = ["**/.git/**", "**/.ctx/**", "**/target/**", "**/node_modules/**"]
follow_symlinks = false
```

The model alias stores only the environment-variable name. Export the actual
credential in the process that runs `ctx`:

```sh
export OPENAI_API_KEY="..."
```

The local runtime currently includes an OpenAI Responses adapter. The runtime
interface itself is provider-neutral, so embedding hosts can register other model
providers in code.

### 2. Bind source-scoped retrieval tools

Create `.ctx/tools/project-search.toml`:

```toml
schema_version = 1

[tool]
name = "project.search"
implementation = "builtin.retrieval.search"
description = "Search indexed project files"

[config]
source = "filesystem:project"

[fixed]
limit = 8

[restrictions]
sources = ["filesystem:project"]
max_output_bytes = 65536
```

Create `.ctx/tools/project-get.toml`:

```toml
schema_version = 1

[tool]
name = "project.get"
implementation = "builtin.retrieval.get"
description = "Read one indexed project document"

[config]
source = "filesystem:project"

[restrictions]
sources = ["filesystem:project"]
max_output_bytes = 131072
```

The fixed search limit is removed from the model-visible schema and cannot be
overridden by a tool call. The source restriction is enforced again at runtime,
including when a document is fetched by ID.

You can also bind `builtin.scoped_file_read` to a concrete workspace-relative
directory. It rejects absolute paths, parent traversal, special files, and
symlink escapes. See the repository's `examples/local-agents` directory for a
`docs.read` example.

### 3. Define the agent

Create `.ctx/agents/project-researcher.toml`:

```toml
[agent]
name = "project-researcher"
description = "Answers project questions from indexed evidence"
model = "default"
tools = ["project.search", "project.get"]

[agent.execution]
max_turns = 8
timeout_seconds = 180

[agent.permissions]
mode = "read-only"

[prompt]
system = """
Search project context before answering factual questions. Use project.get when a
search excerpt is not enough. Cite file paths or document titles for factual
claims. If the indexed evidence is incomplete, identify the missing evidence
instead of guessing.
"""
```

Keep the reusable role and evidence policy in `prompt.system`. Put the immediate
job in the input to `ctx agent run`. This keeps one agent useful across many
projects and tasks.

### 4. Validate without side effects

Run validation before creating a database or contacting a provider:

```sh
ctx agent validate
ctx tool bindings validate
ctx agent show project-researcher --json
ctx tool bindings show project.search --json
```

Agent and binding inspection is static. It does not read credentials, start MCP
servers, evaluate privileged Lua modules, open the database, call a model, or
access the network. Secret references appear as references, never values.

With `--config` or `CTX_CONFIG`, Context Harness loads only the `agents` and
`tools` directories beside that config file. Without an explicit config, it
layers global resources below `.ctx/agents` and `.ctx/tools`. Overrides replace a
whole resource and require `override = true`; fields never merge implicitly.

### 5. Initialize, sync, and run

```sh
ctx init
ctx sync filesystem:project
ctx search "architecture" --source "filesystem:project"
ctx agent run project-researcher \
  "Explain the architecture and cite the files you used."
```

If the direct search is empty, fix the connector or sync state before debugging
the agent. Missing context is not evidence that no relevant documents exist.

Interactive runs may request approval for capabilities allowed by the agent but
requiring approval under host policy. `--non-interactive` denies those requests;
it does not auto-approve them.

### 6. Inspect and recover runs

```sh
ctx agent history --limit 20
ctx agent inspect <run-id>
ctx agent resume <run-id>
```

History records model usage, tool calls, approval decisions, binding identities,
and terminal state without storing credential values or provider response bodies.
Resume rejects a changed agent, model binding, tool schema, fixed configuration,
restriction, or uncertain side effect. MCP-backed runs are non-resumable because
their external session cannot be reconstructed safely.

### Conversation history and memory

Conversation state is retained **within one run**. Each model response, tool call,
and tool result is carried into the next turn. Safe checkpoints preserve those
messages so an interrupted run can continue with `ctx agent resume <run-id>`.

A new `ctx agent run` starts a new conversation containing only the configured
system prompt and the new input. Previous runs are persisted for audit and
inspection, but they are not automatically placed in the new model request and
are not currently exposed as searchable agent memory. Profiles retain no
conversation state at all; their external client owns it.

To carry knowledge between runs today, persist the durable result in a configured
connector source and sync it into Context Harness. Automatic cross-run memory
should be treated as a separate, opt-in feature with explicit retention, scope,
provenance, and deletion controls—not as an implicit replay of every transcript.

### Permissions and approvals

Read-only mode is the safest default:

```toml
[agent.permissions]
mode = "read-only"
```

For a workflow that genuinely needs more authority, replace named mode with
explicit lists:

```toml
[agent.permissions]
allow = ["read_only"]
require_approval = ["workspace_write", "process_execute"]
```

Capabilities absent from both lists are denied. Effective authority is the
intersection of host grants, implementation requirements, binding restrictions,
agent permissions, and runtime policy. A resource can narrow that intersection;
it cannot widen it.

### Compiled, Lua, and MCP tools

Declarative bindings separate a public tool contract from trusted executable
behavior:

- **Compiled Rust:** the embedding host registers a `rust.*` implementation and
  stable version. A resource cannot name a crate or load a library.
- **Privileged Lua:** the host registers a `lua.*` manifest. Preparation requires
  approval before the module is evaluated, and cancellation is cooperative at
  VM instruction boundaries.
- **MCP alias:** a resource refers to `mcp.<server>.<remote-tool>`. Static
  inspection reports remote metadata as unresolved. Authorized startup discovers
  the schema, rejects drift, and preserves normal per-call approval requirements.

All selected tools still pass through the same validation, policy, approval,
cancellation, output-bound, history, and recovery lifecycle.

### Use the setup skill

The repository includes a reusable Codex skill at
`skills/context-harness-agents`. Install it in your personal skill directory:

```sh
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills"
cp -R skills/context-harness-agents \
  "${CODEX_HOME:-$HOME/.codex}/skills/context-harness-agents"
```

Then ask:

```
Use $context-harness-agents to set up a read-only project researcher for this repository.
```

For an empty workspace, the skill includes a non-destructive initializer that
creates the config, agent, and source-scoped bindings. In an existing workspace,
it inspects and minimally updates the current resources instead of overwriting
them.

### More examples

The repository example includes:

- a project researcher using source-scoped search and get;
- a code reviewer that also has a path-scoped documentation reader;
- a complete `.ctx/config.toml` template;
- validation, run, history, and recovery commands.

See `examples/local-agents` or continue with the
[CLI reference](@/docs/reference/cli.md).
