# Local Agent Examples

This directory is a generic starting point for standalone Context Harness agents.
It indexes a project, exposes source-scoped retrieval bindings, and defines two
read-only agents. Nothing assumes a wiki, note-taking app, or particular business
domain.

Copy the example resources into a project's `.ctx` directory:

```sh
mkdir -p .ctx/agents .ctx/tools
cp examples/local-agents/config.toml .ctx/config.toml
cp examples/local-agents/agents/*.toml .ctx/agents/
cp examples/local-agents/tools/*.toml .ctx/tools/
```

Edit `.ctx/config.toml` so the filesystem root, include globs, and OpenAI model ID
fit the project. Keep the API key in `OPENAI_API_KEY`; do not put it in TOML.
The `docs.read` binding requires a `docs` directory; remove that binding and its
entry in `code-reviewer.toml` when the project does not have one.

Validate before any database or provider operation:

```sh
ctx agent validate
ctx tool bindings validate
ctx agent show project-researcher --json
ctx tool bindings show project.search --json
```

Then initialize, ingest, and run:

```sh
ctx init
ctx sync filesystem:project
ctx agent run project-researcher "Explain the architecture and cite the files you used."
ctx agent run code-reviewer "Find the error-handling conventions relevant to this change."
```

The `project.search` and `project.get` aliases are restricted to
`filesystem:project`. `docs.read` is an optional direct file reader restricted to
the `docs` directory.

Use `ctx agent history`, `ctx agent inspect <run-id>`, and
`ctx agent resume <run-id>` for durable execution and recovery. MCP-backed runs
are intentionally non-resumable.
