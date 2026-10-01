# Context Harness Agents Skill

This Codex skill configures and troubleshoots Context Harness standalone local
agents. It can inspect an existing workspace, add model and connector
configuration, create narrowly scoped declarative tool bindings, define agents,
and run static validation.

It is not an agent runtime. After setup, `ctx agent run` executes the configured
agent without involving the skill.

## Install

From a Context Harness checkout, install the complete directory at repository
scope:

```sh
mkdir -p /path/to/project/.codex/skills/context-harness-agents
cp -R skills/context-harness-agents/. \
  /path/to/project/.codex/skills/context-harness-agents/
```

Or install it for the current user:

```sh
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills/context-harness-agents"
cp -R skills/context-harness-agents/. \
  "${CODEX_HOME:-$HOME/.codex}/skills/context-harness-agents/"
```

## Use

Open the target repository in Codex and invoke the skill by name:

```
Use $context-harness-agents to set up a read-only project researcher for this
repository. Index docs and source files, preserve any existing Context Harness
configuration, and validate the result without running the model.
```

Tell Codex the desired role, content root, model or existing model alias, required
actions, and whether it may run the model. Start with read-only permissions and
add broader capabilities only when the workflow requires them.

For an empty workspace, the skill can create:

```text
.ctx/config.toml
.ctx/agents/<agent>.toml
.ctx/tools/<source>-search.toml
.ctx/tools/<source>-get.toml
```

Its initializer refuses to overwrite existing target files. In an existing
workspace, the skill is instructed to inspect first and make minimal edits.

## Learn more

- [Agent setup skill guide](https://parallax-labs.github.io/context-harness/docs/guides/agent-setup-skill/)
- [Manual local-agent configuration](https://parallax-labs.github.io/context-harness/docs/guides/local-agents/)
- [Profiles versus executable agents](https://parallax-labs.github.io/context-harness/docs/guides/agents/)

See [`SKILL.md`](SKILL.md) for the workflow Codex follows and `references/` for
resource schemas and operational troubleshooting.
