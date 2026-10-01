# Operations and Troubleshooting

Read this reference when validating, running, resuming, or diagnosing an agent.

## Static Checks

Run from the workspace root:

```sh
ctx agent validate
ctx agent list --json
ctx agent show <agent> --json
ctx tool bindings validate
ctx tool bindings list --json
ctx tool bindings show <binding> --json
```

These checks are declaration-only. An MCP alias reporting
`remote_metadata: "unresolved"` is valid static output, not proof of a live remote
schema.

## Data and Runtime

```sh
ctx init
ctx sync filesystem:<source>
ctx search "smoke test" --source "filesystem:<source>"
ctx agent run <agent> "Answer one question and cite the evidence you used."
```

Use `--non-interactive` only when denial of every approval-requiring call is the
intended behavior.

Inspect durable state with:

```sh
ctx agent history --limit 20
ctx agent inspect <run-id>
ctx agent resume <run-id>
```

Resume requires the same canonical workspace, agent definition, model binding,
tool identities, and recovery-safe state. Completed runs, uncertain side effects,
changed bindings, and MCP-backed external sessions do not resume.

## Failure Classification

| Symptom | Check |
|---|---|
| Agent or model alias not found | Effective config path and sibling `agents/` directory |
| Unknown tool implementation | Trusted catalog registration or implementation ID typo |
| Retrieval source not granted | Connector name and exact `type:name` source ID |
| Cannot enforce restriction | Binding uses a restriction unsupported by that implementation |
| Tool call denied | Agent permissions, host policy ceiling, and approval result |
| Empty answer or no evidence | Successful sync, source filter, and direct `ctx search` output |
| Provider model not found | Model ID available to the configured provider account |
| Provider rate limited | Provider response and account rate/credit state; do not relabel as a model error |
| Resume rejected | `ctx agent inspect` binding snapshot and current resource versions |

Do not fix missing or stale indexed data by weakening tool restrictions. Repair
the connector or sync state first.
