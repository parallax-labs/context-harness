+++
title = "Use the Agent Setup Skill"
description = "Install and use the context-harness-agents Codex skill to configure local agents safely."
weight = 3
aliases = ["/docs/guides/agent-setup-skill/"]
+++

The repository includes a Codex skill named `context-harness-agents`. It teaches
Codex how to inspect a project, configure Context Harness, create narrowly scoped
tool bindings, define local agents, validate the result, and diagnose common
runtime failures.

The skill is an **authoring assistant**. It configures local agents; it is not an
agent runtime and does not remain involved when you later execute
`ctx agent run`.

### When to use it

Use the skill when you want Codex to:

- add the first local agent to a repository;
- adapt an agent to an existing Context Harness configuration;
- create source-scoped retrieval or path-scoped file tools;
- add another generic role such as a reviewer, release assistant, or incident
  analyst;
- validate or troubleshoot agent resources and tool bindings.

Do not use it to create an MCP profile unless you are intentionally migrating
that profile into an executable local agent. See
[Profiles Overview](@/docs/profiles/overview.md) for the distinction.

### 1. Install the skill

The skill directory is
[`skills/context-harness-agents`](https://github.com/parallax-labs/context-harness/tree/main/skills/context-harness-agents)
in the Context Harness repository. Choose one installation scope.
[Codex supports](https://developers.openai.com/blog/eval-skills) repository-scoped
skills under `.codex/skills` and user-scoped skills under `~/.codex/skills`.

For only the current repository, copy it into the repository-scoped Codex skill
directory:

```sh
mkdir -p .codex/skills/context-harness-agents
cp -R /path/to/context-harness/skills/context-harness-agents/. \
  .codex/skills/context-harness-agents/
```

For every repository you use with Codex, install it in your personal skill
directory:

```sh
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills/context-harness-agents"
cp -R /path/to/context-harness/skills/context-harness-agents/. \
  "${CODEX_HOME:-$HOME/.codex}/skills/context-harness-agents/"
```

Keep the entire directory together. In addition to `SKILL.md`, it contains the
safe workspace initializer and detailed resource and operations references.
Codex discovers a skill from the `name` and `description` in `SKILL.md`; using
the `$context-harness-agents` name explicitly is the most predictable way to
activate it.

### 2. Prepare the request

Before invoking the skill, decide:

| Choice | Example | Where it is stored |
|---|---|---|
| Agent role | Project researcher | `.ctx/agents/project-researcher.toml` |
| Context root | `docs` or the repository root | `.ctx/config.toml` connector |
| Model | An OpenAI model available to your account | `.ctx/config.toml` model alias |
| Required actions | Search and retrieve only | `.ctx/tools/*.toml` and agent permissions |
| Runtime bounds | 8 turns, 180 seconds | Agent execution settings |

The built-in CLI runtime currently provides an OpenAI Responses adapter. Set the
credential in the environment that will run `ctx`; never place the key in TOML:

```sh
export OPENAI_API_KEY="..."
```

If you do not know the exact model ID, tell Codex that instead of guessing one.
The skill will preserve a usable existing model alias or ask for the missing
choice.

### 3. Invoke the skill

Start Codex in the repository you want to configure and make the intended
authority explicit. For example:

```
Use $context-harness-agents to set up a read-only project researcher for this
repository. Index docs and Rust source files. Use the existing model alias if one
is configured, otherwise ask me for the model ID. Validate everything, but do not
run the model yet.
```

Other useful requests include:

```
Use $context-harness-agents to add a code-review agent. It may search indexed
project files and read files under docs, but it must not edit files or run
processes.
```

```
Use $context-harness-agents to inspect my existing local-agent configuration,
explain why project.search is denied, and make only the smallest necessary fix.
```

```
Use $context-harness-agents to create a release-preparation agent using my
existing connector and model. Show me the proposed tools and permissions before
adding any write or process capability.
```

Codex may also select the skill from a matching natural-language request, but
the `$` form makes your intent unambiguous.

### 4. Review what it creates

For an empty workspace, the bundled initializer can create:

```
.ctx/
├── config.toml
├── agents/
│   └── project-researcher.toml
└── tools/
    ├── project-search.toml
    └── project-get.toml
```

The skill must inspect existing resources before editing them. Its initializer
refuses partial setup when any target file already exists, so it cannot silently
replace a project configuration. For an existing workspace, Codex should make
minimal edits rather than run the initializer over it.

Review these relationships:

1. `[models.<alias>]` defines the provider model and credential environment
   variable.
2. `[connectors.<type>.<name>]` defines what is ingested. Its source ID is
   `<type>:<name>`, such as `filesystem:project`.
3. Each `.ctx/tools/*.toml` file gives a trusted implementation a public name and
   narrows its source, path, fixed arguments, and output size.
4. `.ctx/agents/<name>.toml` selects the model alias and only the public tools
   that agent needs.
5. Agent permissions can narrow effective authority but cannot grant authority
   unavailable from the host or implementation.

See [Build Local Agents](@/docs/agents/build-local-agents.md) for every field and a
complete manual configuration.

### 5. Validate before runtime work

The skill should run these declaration-only checks first:

```sh
ctx agent validate
ctx tool bindings validate
ctx agent show project-researcher --json
ctx tool bindings list --json
```

They do not open the database, resolve credentials, start MCP servers, evaluate
Lua modules, call a model, or access the network. Confirm that the displayed
model alias, tools, source restrictions, and permissions match your request.

Only then initialize data, sync the connector, and run the agent:

```sh
ctx init
ctx sync filesystem:project
ctx search "smoke test" --source filesystem:project
ctx agent run project-researcher \
  "Explain this project's architecture and cite the evidence you used."
```

If search returns no evidence, repair the connector or sync state before changing
the agent. Empty or stale context is not an agent prompt problem.

### Run the initializer directly

You normally ask Codex to use the skill so it can inspect and adapt the
workspace. For a known-empty workspace, you can run the bundled deterministic
initializer yourself from the skill directory:

```sh
python3 scripts/bootstrap_workspace.py \
  --workspace /path/to/project \
  --agent project-researcher \
  --content-root docs \
  --source project \
  --provider-model gpt-5-mini \
  --dry-run
```

Remove `--dry-run` only after reviewing the four target paths. The script accepts
`--model-alias` when the alias should not be `default`. It rejects absolute or
escaping content roots, invalid identifiers, and every overwrite.

### What a successful skill run reports

Ask Codex to finish with:

- every created or changed path;
- the effective configuration path and model alias;
- the exact connector source ID;
- the agent's public tools and effective permission mode;
- validation results;
- the sync and run commands;
- any blocker classified as missing credentials, unavailable model, empty index,
  policy denial, or provider rate limit.

That report makes the generated setup reviewable before you trust it with a live
model or broader permissions.
