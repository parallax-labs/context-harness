+++
title = "Connect the repositories behind your change"
description = "Configure related repositories in one index and make their source and documentation searchable from your coding assistant."
weight = 1
+++

Start with two or three repositories you routinely consult together. This guide uses one Context Harness configuration and one index; you do not need a separate Context Harness workspace for each repository.

## 1. Configure existing checkouts

Create a configuration file outside the repositories you are indexing, for
example `~/context-harness/config/ctx.toml`. Replace each absolute path below
with an existing local checkout. Keep the database and state paths in a
directory you control. The example uses keyword search and needs no model or API
key.

```toml
[db]
path = "/Users/you/context-harness/data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "disabled"

[retrieval]
final_limit = 12
hybrid_alpha = 0.6

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem.order_processing]
root = "/Users/you/src/order-processing"
include_globs = ["**/*.rs", "**/*.ts", "**/*.tsx", "**/*.json", "**/*.md", "**/AGENTS.md", "**/CLAUDE.md"]
exclude_globs = ["**/.git/**", "**/target/**", "**/node_modules/**", "**/dist/**", "**/build/**", "**/generated/**"]

[connectors.filesystem.event_framework]
root = "/Users/you/src/event-framework"
include_globs = ["**/*.rs", "**/*.ts", "**/*.tsx", "**/*.json", "**/*.md", "**/AGENTS.md", "**/CLAUDE.md"]
exclude_globs = ["**/.git/**", "**/target/**", "**/node_modules/**", "**/dist/**", "**/build/**", "**/generated/**"]

[connectors.filesystem.lake_contracts]
root = "/Users/you/src/lake-contracts"
include_globs = ["**/*.sql", "**/*.json", "**/*.md", "**/AGENTS.md", "**/CLAUDE.md"]
exclude_globs = ["**/.git/**", "**/target/**", "**/node_modules/**", "**/dist/**", "**/build/**", "**/generated/**"]
```

The paths above are examples: change `/Users/you` and repository names to match
your machine. Include globs select source and Markdown guidance files; exclude
globs skip common vendor, build, and generated directories. Add or remove file
patterns to fit the languages and material you need. Indexing `AGENTS.md` or
`CLAUDE.md` makes its text searchable. It does not automatically apply those
instructions to your coding assistant; load the applicable repository and
directory guidance for the task.

If you prefer not to have a local checkout for one source, add a named Git
connector to the same configuration, replacing the URL with an accessible
repository:

```toml
[connectors.git.lake_pipeline]
url = "https://github.com/your-org/lake-pipeline.git" # Placeholder: replace with an accessible repository URL.
branch = "main"
include_globs = ["transforms/**/*.sql", "docs/**/*.md", "README.md"]
exclude_globs = ["**/generated/**", "**/vendor/**"]
shallow = true
```

A remote clone does not include uncommitted local edits. Use the
[connector reference](@/docs/connectors/built-in.md) for Git options.

## 2. Initialize and sync the index

Use the same explicit configuration path for every command. Replace the example
path with the absolute path to the file you created:

```sh
ctx --config /Users/you/context-harness/config/ctx.toml init
ctx --config /Users/you/context-harness/config/ctx.toml sync all
ctx --config /Users/you/context-harness/config/ctx.toml sources
```

Each filesystem connector is a named source. Sync again when you want to refresh
the indexed material; search does not automatically read changed files.

## 3. Search across repositories

Search the shared collection from the CLI:

```sh
ctx --config /Users/you/context-harness/config/ctx.toml search "OrderCreated fulfillment_region"
```

You can narrow a search to a named source with `--source filesystem:event_framework`.
See [quick start](@/docs/getting-started/quick-start.md) for the single-source
path and [search reference](@/docs/reference/search.md) for search options.

## 4. Connect Claude Code over MCP

Start the HTTP MCP server in one terminal. Keep it running while your client
uses the connection:

```sh
ctx --config /Users/you/context-harness/config/ctx.toml serve mcp
```

In another terminal, register the endpoint once at Claude Code user scope so it
is available across your projects:

```sh
claude mcp add --transport http --scope user context-harness http://127.0.0.1:7331/mcp
claude mcp list
```

The CLI registration configures the client; the Context Harness server still
needs to be running and reachable. Start a new Claude Code session or inspect
`/mcp` to confirm the connection. See the official
[Claude Code MCP guide](https://code.claude.com/docs/en/mcp) for client options.
Other MCP-compatible clients can use the same URL with their own registration
steps.

Give your coding assistant a focused investigation prompt, for example:

> Trace how `fulfillment_region` would move from the order producer through the
> event framework into the data lake. Search the configured repositories, cite
> the files you use, check the applicable repository instructions, and list the
> implementation and compatibility tests I should inspect. Do not edit files.

Connecting MCP makes tools available. It does not ensure the client calls them,
searches every source, or loads each repository's instructions. Verify source
coverage and inspect current files before relying on a result.

## Optional: add local semantic search

Keyword search is ready with the configuration above. To add local embeddings,
replace `[embedding]` with:

```toml
[embedding]
provider = "local"
```

The local embedding model downloads on first use and is reused from the local
model cache afterward. Run `embed pending` after syncing to create embeddings
for chunks that do not have them:

```sh
ctx --config /Users/you/context-harness/config/ctx.toml embed pending
```

That warm model-cache reuse is separate from Claude Code input tokens: retrieved
source text the client sends is still part of its model request. Local embeddings
do not mean the assistant's conversation or
other configured providers stay local. A later source sync refreshes indexed
content; it does not make search a live filesystem read.

## More than one organization context?

For separate indexes and configurations, register one workspace per context;
each workspace can contain several repository sources. This shared-server mode
does not use the guide's `--config` option or `CTX_CONFIG`; do not combine them
with `--workspaces`. The
[multi-workspace MCP routing runbook](https://github.com/parallax-labs/context-harness/blob/main/docs/runbook/0018-multi-workspace-mcp-routing.md)
explains registration and the shared endpoint:

```sh
ctx serve mcp --workspaces
```

Initialize and sync each workspace independently before starting the router.
The router does not refresh their stores. Built-in requests select a workspace
or use the configured default; searching across workspaces is explicit. Search
results use workspace-qualified document IDs, while retrieval selects a single
workspace using that workspace's ID. The router currently covers built-in tools, not complete
workspace-local tool and profile routing. Workspace selection is not an access
control boundary; use this mode only in a trusted local environment.
