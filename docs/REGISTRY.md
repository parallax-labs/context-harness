# Extension Registries — Specification & Guide

This document specifies the extension registry system for Context Harness.
Registries are Git-backed repositories of Lua connectors, tools, and agents
that can be installed, searched, overridden, and shared.

**Status:** Implemented
**Author:** Parker Jones
**Created:** 2026-02-25
**Depends on:** `config.rs`, `connector_script.rs`, `tool_script.rs`, `agent_script.rs`, `server.rs`

---

## 1. Motivation

Context Harness supports custom connectors, tools, and agents via Lua scripts,
but users have to write them from scratch. A registry system lets the community
share ready-to-use extensions — Jira connectors, Confluence connectors, Slack
connectors, summarization tools, incident response agents — installable with
a single command.

This is modeled after [cheat/cheat](https://github.com/cheat/cheat)'s
community cheatsheet system, adapted for Context Harness's Lua extension
model.

### Design Goals

- **Zero compilation** — extensions are Lua scripts, not compiled code
- **Git-native** — registries are plain Git repos, updated with `git pull`
- **Precedence** — multiple registries with overrides (community < company < personal < project-local)
- **Discoverable** — search, list, and inspect extensions before installing
- **Opt-in activation** — connectors need credentials, so they aren't auto-active

---

## 2. Architecture

```
ctx.toml                    Registry (Git repo)
┌──────────────────┐        ┌───────────────────────────┐
│ [registries.     │        │ registry.toml             │
│   community]     │───────▶│ connectors/               │
│ url = "..."      │  clone │   jira/connector.lua      │
│ path = "~/.ctx/  │        │   confluence/connector.lua│
│   registries/    │        │ tools/                    │
│   community"     │        │   summarize/tool.lua      │
│ readonly = true  │        │ agents/                   │
└──────────────────┘        │   runbook/agent.lua       │
                            └───────────────────────────┘
```

### Precedence Order

Extensions are resolved with later entries overriding earlier ones:

```
1. Explicit ctx.toml entries    (highest — always wins)
2. .ctx/ project-local           ↑
3. Personal registry (writable)  │
4. Company registry (readonly)   │
5. Community registry (readonly) (lowest)
```

If two registries define `connectors/jira`, the higher-precedence one wins.
Explicit `[connectors.script.jira]` entries in `ctx.toml` always take
priority over any registry version.

---

## 3. Configuration

### 3.1 Registry Config

Add `[registries.<name>]` sections to `ctx.toml`:

```toml
[registries.community]
url = "https://github.com/context-harness/registry.git"
branch = "main"
path = "~/.ctx/registries/community"
readonly = true
auto_update = true

[registries.company]
url = "git@github.com:myorg/ctx-extensions.git"
branch = "main"
path = "~/.ctx/registries/company"
readonly = true
auto_update = true

[registries.personal]
path = "~/.ctx/extensions"
readonly = false
```

### Config Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `url` | string | No | — | Git repository URL. Omit for local-only registries. |
| `branch` | string | No | `"main"` | Git branch or tag to track. |
| `path` | string | Yes | — | Local filesystem path. Supports `~/`. |
| `readonly` | bool | No | `false` | If `true`, extensions can't be edited in place. |
| `auto_update` | bool | No | `false` | If `true`, `ctx registry update` pulls this registry. |

### 3.2 Project-Local Extensions

A `.ctx/` directory in the current working directory (or any ancestor) is
auto-discovered as a project-local registry. No config entry needed.

```
my-project/
  .ctx/
    connectors/
      internal-api/
        connector.lua
    tools/
      project-lint/
        tool.lua
    agents/
      qa-helper/
        agent.lua
  src/
  ...
```

The `.ctx/` directory:
- Has the **highest precedence** (overrides all configured registries)
- Is **writable** (readonly = false)
- Does not require a `registry.toml` — extensions are discovered by directory structure
- Is available from any subdirectory within the project

---

## 4. Registry Layout

### 4.1 Manifest (`registry.toml`)

Every registry should have a `registry.toml` manifest at its root:

```toml
[registry]
name = "community"
description = "Official Context Harness community extensions"
url = "https://github.com/context-harness/registry"
min_version = "0.3.0"

[connectors.jira]
description = "Index Jira issues and comments"
path = "connectors/jira/connector.lua"
tags = ["atlassian", "project-management"]
required_config = ["url", "project_key", "api_token"]
host_apis = ["http", "json", "env"]

[connectors.confluence]
description = "Index Confluence spaces and pages"
path = "connectors/confluence/connector.lua"
tags = ["atlassian", "documentation"]
required_config = ["url", "space_key", "api_token"]
host_apis = ["http", "json", "env"]

[tools.summarize]
description = "Summarize a document using the knowledge base"
path = "tools/summarize/tool.lua"
tags = ["llm", "analysis"]
host_apis = ["http", "json", "context"]

[agents.runbook]
description = "Incident response agent with runbook context"
path = "agents/runbook/agent.lua"
tags = ["ops", "incident-response"]
tools = ["search", "get"]
```

### Extension Entry Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `description` | string | No | One-line description for discovery. |
| `path` | string | Yes | Relative path from registry root to the script file. |
| `tags` | list | No | Tags for filtering and discovery. |
| `required_config` | list | No | Config keys the extension needs (connectors). |
| `host_apis` | list | No | Lua host APIs used by the extension. |
| `tools` | list | No | Tools this agent exposes (agents only). |

### 4.2 Directory Structure

```
registry.toml
LICENSE
CONTRIBUTING.md
connectors/
  jira/
    connector.lua          # the Lua script
    config.example.toml    # example config snippet for ctx registry add
    README.md              # usage documentation
  confluence/
    connector.lua
    config.example.toml
    README.md
tools/
  summarize/
    tool.lua
    README.md
  related-docs/
    tool.lua
    README.md
agents/
  runbook/
    agent.lua
    README.md
  code-reviewer/
    agent.toml
    README.md
```

### 4.3 `config.example.toml`

Each extension can include a `config.example.toml` that `ctx registry add`
uses to scaffold the config entry:

```toml
# config.example.toml for connectors/jira
path = "~/.ctx/registries/community/connectors/jira/connector.lua"
timeout = 600
url = "${JIRA_URL}"
api_token = "${JIRA_API_TOKEN}"
project_key = ""  # TODO: set this
```

### 4.4 Registries Without a Manifest

Registries without a `registry.toml` (including `.ctx/` project-local
directories) are supported via automatic directory scanning. Extensions
are discovered by convention:

- `connectors/<name>/connector.lua`
- `tools/<name>/tool.lua`
- `agents/<name>/agent.lua` or `agents/<name>/agent.toml`

---

## 5. CLI Commands

### 5.1 `ctx registry list`

Show configured registries and all available extensions.

```
$ ctx registry list

Registries:

  community — ~/.ctx/registries/community (git) [readonly]
    6 connectors, 3 tools, 2 agents
  personal — ~/.ctx/extensions
    1 connectors, 0 tools, 0 agents

Available extensions:

  agents:
    code-reviewer — Reviews code against conventions [ops] (from: community)
    runbook — Incident response agent [ops, sre] (from: community)
  connectors:
    confluence — Index Confluence spaces [atlassian] (from: community)
    github-issues — Index GitHub issues [github] (from: community)
    jira — Index Jira issues [atlassian, pm] (from: community)
    jira — Custom Jira connector (from: personal)
    ...
  tools:
    related-docs — Find related documents (from: community)
    summarize — Summarize a document [llm] (from: community)
```

### 5.2 `ctx registry install [name]`

Clone git-backed registries that aren't yet present on disk.

```
$ ctx registry install
Cloning registry 'community' from https://github.com/context-harness/registry.git...
  Installed: 6 connectors, 3 tools, 2 agents
```

### 5.3 `ctx registry update [name]`

Pull the latest changes for git-backed registries.

```
$ ctx registry update
Updating community...
  Updated successfully.
Updating company...
  Up to date.
```

Registries with uncommitted changes are skipped with a warning.

### 5.4 `ctx registry search <query>`

Search extensions by name, description, or tags.

```
$ ctx registry search atlassian
Found 2 extensions matching 'atlassian':

  connectors/jira — Index Jira issues [atlassian, pm] (from: community)
  connectors/confluence — Index Confluence spaces [atlassian, docs] (from: community)
```

### 5.5 `ctx registry info <type/name>`

Show details for a specific extension.

```
$ ctx registry info connectors/jira
Extension: connectors/jira
Registry:  community
Script:    ~/.ctx/registries/community/connectors/jira/connector.lua
Description: Index Jira issues and comments
Tags: atlassian, project-management
Required config: url, project_key, api_token
Host APIs: http, json, env

--- README ---

# Jira Connector
...
```

### 5.6 `ctx registry add <type/name>`

Scaffold a config entry in `ctx.toml` for an extension.

```
$ ctx registry add connectors/jira
Added [connectors.script.jira] to config/ctx.toml
Edit config/ctx.toml to set: url, project_key, api_token
```

This appends to your config file:

```toml
[connectors.script.jira]
path = "~/.ctx/registries/community/connectors/jira/connector.lua"
timeout = 600
url = "${JIRA_URL}"
api_token = "${JIRA_API_TOKEN}"
project_key = ""  # TODO: set this
```

### 5.7 `ctx registry override <type/name>`

Copy an extension from a readonly registry to a writable one for
customization.

```
$ ctx registry override connectors/jira
Copied connectors/jira to ~/.ctx/extensions/connectors/jira/
Your version will take precedence over the community registry version.
```

The override can then be edited directly. It takes precedence over the
original because the personal registry is higher-precedence.

### 5.8 `ctx registry init`

Install the community registry on first run.

```
$ ctx registry init
Cloning community extension registry...
Installed: 6 connectors, 3 tools, 2 agents
Added [registries.community] to config/ctx.toml
Run `ctx registry list` to see available extensions.
```

This is also offered interactively during `ctx init` when no registries
are configured and stdin is a terminal.

---

## 6. Auto-Discovery at Server Startup

When `ctx serve mcp` starts, tools and agents from registries are
automatically loaded alongside explicitly configured ones.

### What's auto-discovered

| Type | Auto-active? | Why |
|------|-------------|-----|
| **Tools** | Yes | Tools only access the existing knowledge base via `context.search`, `context.get`, `context.sources`. No external credentials needed. |
| **Agents** | Yes | Same as tools — agents use the knowledge base and scoped tools. |
| **Connectors** | No | Connectors talk to external APIs (Jira, Confluence, Slack) and need credentials. Must be explicitly activated via `ctx registry add`. |

### Precedence at startup

1. Explicit `[tools.script.*]` / `[agents.script.*]` entries in `ctx.toml` — always win
2. Registry extensions — loaded in precedence order; later registries override earlier
3. Built-in tools (search, get, sources) — always present

If a registry tool has the same name as an explicitly configured tool,
the explicit config wins and the registry version is skipped.

---

## 7. Creating a Registry

### 7.1 For Your Team

Create a Git repository with the standard directory structure:

```bash
mkdir my-org-extensions && cd my-org-extensions
git init

# Create manifest
cat > registry.toml << 'EOF'
[registry]
name = "myorg"
description = "Internal Context Harness extensions for MyOrg"

[connectors.internal-api]
description = "Index internal API documentation"
path = "connectors/internal-api/connector.lua"
tags = ["internal"]
required_config = ["base_url", "api_key"]
host_apis = ["http", "json", "env"]
EOF

# Create connector
mkdir -p connectors/internal-api
# ... write connector.lua and config.example.toml

git add . && git commit -m "Initial extensions"
git remote add origin git@github.com:myorg/ctx-extensions.git
git push -u origin main
```

Then add to your team's `ctx.toml`:

```toml
[registries.myorg]
url = "git@github.com:myorg/ctx-extensions.git"
path = "~/.ctx/registries/myorg"
readonly = true
auto_update = true
```

### 7.2 For the Community

Contributions to the official community registry follow the same
structure. The community registry will be hosted at
`https://github.com/context-harness/registry`.

Contribution workflow:

1. Fork the registry repository
2. Add your extension in `<type>/<name>/` with script, README, and `config.example.toml`
3. Update `registry.toml` with the new entry
4. Submit a pull request

---

## 8. Walkthrough: From Install to Usage

### Step 1: Install the community registry

```bash
ctx registry init --config ./config/ctx.toml
```

Or during first run:

```
$ ctx init --config ./config/ctx.toml
Database initialized successfully.
Would you like to install the community extension registry? [Y/n] y
Cloning community extension registry...
Installed: 6 connectors, 3 tools, 2 agents
```

### Step 2: Browse available extensions

```bash
ctx registry list --config ./config/ctx.toml
ctx registry search jira --config ./config/ctx.toml
ctx registry info connectors/jira --config ./config/ctx.toml
```

### Step 3: Add a connector

```bash
ctx registry add connectors/jira --config ./config/ctx.toml
# Edit ctx.toml to fill in credentials
vim config/ctx.toml
```

### Step 4: Sync and search

```bash
ctx sync script:jira --config ./config/ctx.toml
ctx search "sprint planning" --config ./config/ctx.toml
```

### Step 5: Start the server

Registry tools and agents are automatically available:

```bash
ctx serve mcp --config ./config/ctx.toml
# Tools from registries appear in GET /tools/list
# Agents from registries appear in GET /agents/list
```

### Step 6: Customize an extension

```bash
ctx registry override connectors/jira --config ./config/ctx.toml
# Edit the override
vim ~/.ctx/extensions/connectors/jira/connector.lua
```

### Step 7: Keep registries updated

```bash
ctx registry update --config ./config/ctx.toml
```

---

## 9. Data Model

### RegistryManifest

```rust
pub struct RegistryManifest {
    pub registry: RegistryMeta,
    pub connectors: HashMap<String, ExtensionEntry>,
    pub tools: HashMap<String, ExtensionEntry>,
    pub agents: HashMap<String, ExtensionEntry>,
}
```

### ExtensionEntry

```rust
pub struct ExtensionEntry {
    pub description: String,
    pub path: String,              // relative to registry root
    pub tags: Vec<String>,
    pub required_config: Vec<String>,
    pub host_apis: Vec<String>,
    pub tools: Vec<String>,        // agents only
}
```

### ResolvedExtension

```rust
pub struct ResolvedExtension {
    pub name: String,
    pub kind: String,              // "connector", "tool", or "agent"
    pub script_path: PathBuf,
    pub registry_name: String,
    pub entry: ExtensionEntry,
}
```

### RegistryManager

```rust
pub struct RegistryManager {
    registries: Vec<LoadedRegistry>,
}

impl RegistryManager {
    pub fn from_config(config: &Config) -> Self;
    pub fn list_all() -> Vec<ResolvedExtension>;
    pub fn resolve(extension_id: &str) -> Option<ResolvedExtension>;
    pub fn list_connectors() -> Vec<ResolvedExtension>;
    pub fn list_tools() -> Vec<ResolvedExtension>;
    pub fn list_agents() -> Vec<ResolvedExtension>;
    pub fn writable_path() -> Option<&Path>;
    pub fn registries() -> Vec<RegistryInfo>;
}
```

---

## 10. Stability

The public contract is defined by:
- `REGISTRY.md` (this document)
- The `[registries]` config schema
- The `ctx registry` CLI subcommands
- The `registry.toml` manifest format

Changes to the `RegistryManifest` structure or CLI interface constitute
breaking changes and require a major version bump.
