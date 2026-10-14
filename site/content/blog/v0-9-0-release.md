+++
title = "v0.9.0: From Local Knowledge to Local Agents"
description = "Run bounded agents over your project knowledge, inspect their work, and embed durable execution in your own applications."
date = 2026-10-14

[taxonomies]
tags = ["release"]
+++

Context Harness v0.9.0 is out. This release brings the local knowledge index and
a durable agent runtime together: your tools can retrieve project context, and
Context Harness can now use that context to carry out bounded work itself.

The practical difference is ownership. An external client can still use Context
Harness for search and reusable prompt profiles. Or you can ask Context Harness
to run the model-and-tool loop, enforce permissions, and retain an inspectable
record of what happened.

### Agents that work inside your project

Define an agent's role, model, tools, permissions, and limits in a standalone TOML
resource. Run it from the CLI with OpenAI or a local Ollama model, then inspect
its model turns, tool calls, approvals, usage, and artifacts.

Tools can be restricted to particular sources and paths. Higher-risk operations
can require approval. Static resource validation does not contact providers,
resolve secrets, invoke tools, open run databases, or start processes.

Start with a project researcher or code reviewer and a small, read-only tool set.
The [Build Local Agents guide](@/docs/agents/build-local-agents.md) walks through
the setup, and `examples/local-agents` contains complete starting configurations.

### Durable work, explicit limits

Agent execution now includes durable task submission and foreground workers,
typed outcomes, cumulative run budgets, structured working state, and explicit
continuation of suspended runs. A task that needs input can stop with a recorded
reason and continue within the same run when the host supplies that input.

Bounded context projection keeps the model request within configured limits
without deleting durable history. Recoverable tool errors can become observations
for the next model turn, with safeguards against repeating the same failed call.

Recovery remains conservative: changed definitions, uncertain side effects, or
incompatible checkpoints can prevent continuation. This is not a promise to
replay every interrupted operation automatically.

### An execution foundation for your application

The public Rust APIs let a trusted host register compiled model and tool
implementations, assemble an agent host, submit durable tasks, and run workers.
Configuration selects registered implementations; it does not dynamically load
provider code.

Applications can also refresh specific changed documents through the targeted
refresh API, without rediscovering an entire connector source. The host keeps
ownership of its events, files, publication rules, and user experience.

See [Embed an Agent Host](@/docs/agents/embed-an-agent-host.md) for the public
contracts and integration fixture. Context Harness supplies generic execution
and retrieval capabilities, not application-specific wiki or hook behavior.

### Retrieval and profiles still have their place

This release also adds multi-workspace MCP routing, including bounded fan-out
search, and improves natural-language keyword matching. SQLite remains the
canonical store; the vector index remains a rebuildable accelerator.

Reusable prompt personas are now presented as **profiles**, distinct from
executable agents. Existing legacy profile aliases remain supported. Use a
profile when your editor or chat client owns the conversation; use an agent when
Context Harness should own execution.

### Download and upgrade

Get the archives and matching SHA256 files from the
[v0.9.0 release](https://github.com/parallax-labs/context-harness/releases/tag/v0.9.0).
Downloads cover Linux x86_64, Linux musl, Linux aarch64, Intel macOS, Apple Silicon
macOS, and Windows x86_64.

For example, in a fresh directory on Apple Silicon:

```sh
curl -fLO https://github.com/parallax-labs/context-harness/releases/download/v0.9.0/ctx-macos-aarch64.tar.gz
curl -fLO https://github.com/parallax-labs/context-harness/releases/download/v0.9.0/ctx-macos-aarch64.tar.gz.sha256
shasum -a 256 -c ctx-macos-aarch64.tar.gz.sha256
tar xzf ctx-macos-aarch64.tar.gz
./ctx --version
```

Both Rust crates are versioned `0.9.0`. To build from a clean source checkout at
the release tag, use `cargo install --path crates/context-harness --locked --force`.

**Finish active runs before upgrading.** Some tool-binding implementation
identities include the Cargo package version. Runs created with an older
development build may therefore be refused by checkpoint compatibility checks
after the upgrade. Retaining run history does not guarantee cross-version resume
compatibility; those safety checks have not been weakened.

### Release verification

The release passed main-branch CI and all six platform release builds. All six
downloaded archives matched their published SHA256 checksums. The downloaded
Apple Silicon binary reported `ctx 0.9.0` and successfully displayed `--help`.
The other platform binaries were built in CI but were not executed in this local
smoke test.

For the complete change list, see the
[v0.8.0 to v0.9.0 comparison](https://github.com/parallax-labs/context-harness/compare/v0.8.0...v0.9.0).

The best first step is small: index one project's knowledge, configure a
read-only agent, and ask one question whose answer you can verify.
