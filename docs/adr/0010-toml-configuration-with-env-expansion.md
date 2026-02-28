# ADR-0010: TOML Configuration with Environment Variable Expansion

**Status:** Accepted
**Date:** Retroactive

## Context

Context Harness requires a configuration file that defines:

- Database path and connection settings
- Chunking parameters (max tokens, overlap)
- Embedding provider and model settings
- Retrieval tuning (alpha, candidate pool sizes, limits)
- Server bind address
- Connector instances (filesystem paths, Git URLs, S3 buckets)
- Tool scripts and their settings
- Agent definitions (inline and scripted)
- Extension registry URLs

The format must be human-readable, support comments (for self-documenting
configs), handle nested structures (connector instances), and allow secrets
(API tokens) without hardcoding them in the file.

## Decision

Use **TOML** as the configuration format with a single file (default
`config/ctx.toml`, overridable via `--config`).

### Structure

```toml
[db]
path = "./data/ctx.sqlite"

[chunking]
max_tokens = 700
overlap_tokens = 80

[embedding]
provider = "local"

[retrieval]
hybrid_alpha = 0.6
final_limit = 12

[server]
bind = "127.0.0.1:7331"

[connectors.filesystem.docs]
root = "./docs"
globs = ["**/*.md"]

[connectors.git.platform]
url = "https://github.com/org/repo.git"
branch = "main"

[connectors.s3.reports]
bucket = "my-bucket"
prefix = "reports/"

[tools.script.create_ticket]
path = "tools/create-ticket.lua"
api_token = "${JIRA_API_TOKEN}"

[agents.inline.reviewer]
description = "Reviews code"
tools = ["search", "get"]
system_prompt = "You are a code reviewer..."

[registries.community]
url = "https://github.com/org/ctx-registry.git"
```

### Environment Variable Expansion

Values containing `${VAR_NAME}` are expanded from the process environment
at config load time. This allows secrets to be stored in environment
variables, `.env` files, or secret managers rather than in the config file.

### Config-Free Commands

Commands that do not require a running system (`completions`, `connector init`,
`tool init`, `agent init`, `registry init`) can execute without a valid config
file, avoiding a chicken-and-egg problem during initial setup.

## Alternatives Considered

**YAML.** Widely used in DevOps tooling but has well-known problems:
ambiguous type coercion (`yes` → boolean, `3.10` → float), complex
specification, and surprising indentation rules. TOML avoids all of these
with explicit types and simpler syntax.

**JSON.** Machine-readable but does not support comments, making
self-documenting configs impossible. Multi-line strings (system prompts) are
awkward. JSON5 addresses some issues but has limited Rust library support.

**dotenv (`.env` files only).** Flat key-value pairs with no nesting. Cannot
represent the hierarchical structure needed for multiple connector instances,
agent definitions, or retrieval parameters. Suitable for secrets but not for
full configuration.

**HCL (HashiCorp Configuration Language).** Powerful and supports blocks well,
but uncommon in the Rust ecosystem. Available crates are less mature than
`toml`. Would add a learning curve for users unfamiliar with Terraform.

**CLI flags only.** Possible for simple tools but unmanageable for the number
of settings Context Harness requires. A config file is essential for
reproducible setups.

## Consequences

- TOML's comment support enables self-documenting configuration files. The
  example config (`config/ctx.example.toml`) includes inline documentation
  for every setting.
- Named instances via dotted keys (`[connectors.filesystem.docs]`) allow
  multiple connectors of the same type with distinct configurations — a
  core requirement.
- `${VAR_NAME}` expansion keeps secrets out of config files without
  requiring a separate secret management system. Users can use `.env` files,
  shell exports, or CI secret injection.
- TOML's multi-line string literals (`"""..."""`) make agent system prompts
  readable directly in the config file.
- The `serde` + `toml` crate combination provides type-safe deserialization
  with clear error messages when config keys are wrong or missing.
- Adding new config sections for future features follows the established
  pattern of adding a new TOML table.
