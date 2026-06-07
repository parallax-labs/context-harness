# RUNBOOK-0018: Chat via CLI

**Status:** Draft
**Date:** 2026-03-03
**Author:** Parker Jones
**Last Verified:** 2026-03-03

## Purpose

Use this runbook to run direct chat inference from the `ctx` binary, including grounded and ungrounded modes, streaming output, and session transcript save/load workflows.

## Prerequisites

- Context Harness CLI installed and available as `ctx`.
- Workspace initialized with indexed content if using grounded mode.
- `[llm]` provider configuration present in config file.
- Required provider credentials available in environment variables.

## Steps

1. Confirm your workspace config path and that `ctx` can read it.

   ```bash
   ctx --config ./config/ctx.toml sources
   ```

   Expected output: source status table is printed without config errors.

2. Run a single-turn ungrounded chat request.

   ```bash
   ctx --config ./config/ctx.toml chat "Summarize what Context Harness does" --mode ungrounded
   ```

   Expected output: assistant response is printed and command exits with status 0.

3. Run a single-turn grounded chat request.

   ```bash
   ctx --config ./config/ctx.toml chat "How do we deploy MCP for Cursor?" --mode grounded --limit 5
   ```

   Expected output: retrieval summary is shown, then assistant response grounded in indexed docs.

4. Start an interactive chat session.

   ```bash
   ctx --config ./config/ctx.toml chat --mode grounded --source docs
   ```

   Expected output: REPL starts; each prompt streams a response. End with `Ctrl-D`.

5. Save a transcript during a single-turn run.

   ```bash
   ctx --config ./config/ctx.toml chat "List docs for extension authoring" --save-session ./data/chat-session.jsonl
   ```

   Expected output: response prints and `./data/chat-session.jsonl` contains JSONL records.

6. Resume from a prior transcript.

   ```bash
   ctx --config ./config/ctx.toml chat "Continue from previous context" --load-session ./data/chat-session.jsonl
   ```

   Expected output: response reflects earlier session context.

7. Use JSON output for automation.

   ```bash
   ctx --config ./config/ctx.toml chat "What is our release flow?" --json --no-stream
   ```

   Expected output: valid JSON object with response text and metadata.

## Verification

- `ctx chat` works in both single-turn and interactive modes.
- Grounded mode includes retrieval summary and context-aware responses.
- Session file exists and appends valid JSONL lines.
- `--json` mode returns parseable JSON.
- Command exits non-zero only for actionable runtime/config/provider errors.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `config_error` for provider/model | `[llm]` section missing or incomplete | Add provider/model in config or pass `--provider`/`--model` flags |
| `auth_error` | Missing/invalid API key env var | Export correct credential variable referenced by config |
| `provider_error` | Upstream API returned non-2xx | Check provider status, endpoint URL, and model availability |
| `timeout_error` | Provider request exceeded timeout | Increase timeout in config and retry |
| `context_error` in grounded mode | Retrieval failed or workspace not initialized | Run `ctx init` and `ctx sync all`, then retry |

## Rollback

This runbook is non-destructive. If an invocation fails:

1. Stop the session (`Ctrl-C` or `Ctrl-D`).
2. Re-run with `--mode ungrounded` to isolate retrieval issues.
3. Re-run without `--stream` to isolate streaming transport issues.
4. Remove or archive broken transcript files before retrying.
