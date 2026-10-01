# Resource Schemas

Read this reference when creating or changing Context Harness model, agent, or
tool resources.

## Workspace Layout

```text
.ctx/
|-- config.toml
|-- agents/
|   `-- <agent>.toml
`-- tools/
    |-- <source>-search.toml
    `-- <source>-get.toml
```

With an explicit config, agent and tool resources are loaded only from the
`agents` and `tools` directories beside that config. Without one, Context Harness
layers global resources below workspace `.ctx` resources. A higher-layer resource
must set `override = true` to replace a lower-layer definition; resources never
merge field by field.

## Model and Connector

The built-in local runtime currently supplies an OpenAI Responses adapter. The
alias belongs in the effective config and references a credential environment
variable rather than a value:

```toml
[models.default]
provider = "openai"
model = "gpt-5-mini"
api_key_env = "OPENAI_API_KEY"

[connectors.filesystem.project]
root = "docs"
include_globs = ["**/*.md", "**/*.txt"]
exclude_globs = ["**/.git/**", "**/target/**", "**/node_modules/**"]
follow_symlinks = false
```

The connector above has the exact source ID `filesystem:project`.

## Standalone Agent

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
Search project context before answering factual questions. Use project.get when
a search excerpt is not enough. Cite file paths or document titles and say when
the available evidence does not answer the question.
"""
```

Use explicit permission lists only when named read-only mode is insufficient:

```toml
[agent.permissions]
allow = ["read_only"]
require_approval = ["workspace_write", "process_execute"]
```

Capabilities not present in either list are denied.

## Source-Scoped Retrieval Bindings

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

Fixed properties disappear from the model-visible schema and cannot be
overridden by a tool call. The config source must also appear in
`restrictions.sources`.

## Optional Scoped File Reader

Use this for a concrete subdirectory, not for unrestricted host filesystem
access:

```toml
schema_version = 1

[tool]
name = "project.read_docs"
implementation = "builtin.scoped_file_read"
description = "Read a file under project documentation"

[config]
root = "docs"

[fixed]
max_bytes = 131072

[restrictions]
paths = ["docs"]
max_output_bytes = 131072
```

The configured root must appear in `restrictions.paths`. Absolute paths, parent
traversal, special files, and symlink escapes are rejected at execution.

## Other Implementations

- `rust.*` implementations must be compiled and registered by the embedding
  host. TOML cannot load a crate or shared library.
- `lua.*` implementations require a trusted privileged manifest and approval
  before module evaluation.
- `mcp.<server>.<remote-tool>` aliases validate statically as unresolved. The
  remote schema is resolved only after authorized server startup; drift fails
  before the tool is shown to the model.
