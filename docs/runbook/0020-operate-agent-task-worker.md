# RUNBOOK-0020: Operate a Durable Agent Task Worker

**Status:** Active  
**Last verified:** 2026-10-05  
**Applies to:** SPEC-0026 durable agent tasks

## Purpose

Submit durable standalone-agent work, operate an explicitly started foreground
worker, inspect scheduling state, and request cancellation. Context Harness does
not install or start a background service; operators choose and configure their
service manager.

## Prerequisites

- Run commands from the workspace root containing `.ctx/config.toml` and the
  referenced agent resources.
- Validate the agent and its static model/tool declarations with
  `ctx agent validate` before accepting work.
- Provide provider credentials to the worker process through its environment or
  service manager secret facility. Never place credential values in command
  arguments, unit files, logs, or task payloads.

## Start and verify in the foreground

First start a single foreground worker in a terminal:

```sh
ctx agent worker --worker-id local-worker
```

The worker polls the workspace database and produces no output while idle. It is
non-interactive: any operation requiring approval is denied instead of prompting.
Press Ctrl-C to stop claiming work and begin graceful shutdown of owned executions.

Tune bounded worker behavior only when needed:

```sh
ctx agent worker \
  --max-concurrency 2 \
  --lease-seconds 60 \
  --heartbeat-seconds 20 \
  --poll-ms 500 \
  --max-attempts 3 \
  --shutdown-seconds 30
```

The heartbeat interval must be less than half the lease duration.

## Submit and inspect work

From another terminal, submit work with a caller-controlled idempotency key:

```sh
ctx agent enqueue researcher "Summarize the indexed design" \
  --request-key research-2026-10-05-01 --json
```

Repeating the same key and identical request returns the existing job. Reusing the
key for different work or exceeding `--queue-limit` returns typed JSON and a
nonzero exit status.

List recent jobs and inspect one job's scheduling events:

```sh
ctx agent jobs --limit 20
ctx agent job inspect <task-id> --json
ctx agent job inspect <task-id> --after-sequence 20 --limit 200 --json
```

Listing and inspection are read-only and do not create a missing database. JSON
output never includes worker claim tokens.

Request cancellation with:

```sh
ctx agent job cancel <task-id> --json
```

A queued job becomes terminal immediately. A claimed job records a cancellation
request; its owning worker observes that request and performs bounded graceful
shutdown.

## Service manager examples

Run successfully in the foreground before adapting these examples. Replace paths
and user names for the deployment. Store credential assignments in a separately
protected environment file.

For systemd, a minimal service command is:

```ini
[Service]
Type=simple
User=context-harness
WorkingDirectory=/srv/context-harness/workspace
EnvironmentFile=/etc/context-harness/worker.env
ExecStart=/usr/local/bin/ctx agent worker --worker-id systemd-worker
KillSignal=SIGINT
TimeoutStopSec=45
Restart=on-failure
```

For launchd, use equivalent program arguments and send SIGINT when stopping:

```xml
<key>WorkingDirectory</key>
<string>/Users/ctx/workspace</string>
<key>ProgramArguments</key>
<array>
  <string>/usr/local/bin/ctx</string>
  <string>agent</string>
  <string>worker</string>
  <string>--worker-id</string>
  <string>launchd-worker</string>
</array>
<key>EnvironmentVariables</key>
<dict><!-- non-secret configuration only --></dict>
```

Use the platform's protected secret injection rather than embedding credentials
in the plist.

## Verification

1. Submit a fake-provider fixture job with a unique request key.
2. Confirm `ctx agent jobs` reports `queued`, then a claimed/running state.
3. Confirm `ctx agent job inspect <task-id>` eventually reports terminal task and
   linked run state.
4. Stop the worker with SIGINT and confirm it exits within the configured shutdown
   timeout.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `request_conflict` | The request key already identifies different work | Generate a new key or resend the byte-identical request |
| `queue_full` | The configured queued-work bound was reached | Allow workers to drain the queue before retrying |
| Heartbeat validation error | Heartbeat is not less than half the lease | Lower `--heartbeat-seconds` or raise `--lease-seconds` |
| Approval-required tool fails | Workers never prompt | Adjust trusted policy/resources before enqueueing; do not add an interactive service account prompt |
| No durable agent jobs | The database or task migrations do not exist | Enqueue a validated job or start the worker to initialize migrations |

## Rollback

Stop the service with SIGINT and leave the database intact. Inspect queued and
claimed jobs before cancellation. Removing the service definition does not remove
tasks, run history, checkpoints, or application data.
