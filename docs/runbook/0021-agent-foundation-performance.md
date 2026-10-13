# RUNBOOK-0021: Measure Agent Foundation Performance

**Status:** Active
**Date:** 2026-10-13
**Author:** pjones
**Last Verified:** 2026-10-13

## Purpose

Capture machine-identified baselines for durable queue overhead and local Ollama
cold/warm inference. Results are observations, not portable service-level objectives.

## Prerequisites

- Complete [RUNBOOK-0001](0001-local-dev-setup.md).
- Install Ollama and pull the selected reference model.
- Record host hardware, Ollama version and immutable model ID with every result.

## Steps

1. Record host and model identity.

   ```bash
   system_profiler SPHardwareDataType | grep -E 'Model Name|Model Identifier|Chip|Total Number of Cores|Memory'
   ollama --version
   ollama list
   ```

2. Measure durable queue submission and claim overhead.

   ```bash
   cargo test -p context-harness --test agent_task_store perf_queue_submit_and_claim_overhead -- --ignored --nocapture
   ```

   Expected: one `queue_baseline` line with task count and total/mean milliseconds.

3. Force the model out of memory, then measure a deterministic cold call.

   ```bash
   curl --silent http://127.0.0.1:11434/api/generate -d '{"model":"qwen3.6:latest","keep_alive":0}' >/dev/null
   curl --silent http://127.0.0.1:11434/api/generate -d '{"model":"qwen3.6:latest","prompt":"/no_think Reply with exactly: baseline ok","stream":false,"think":false,"keep_alive":"10m","options":{"temperature":0,"num_predict":16,"seed":42}}' | jq '{response,done_reason,total_duration,load_duration,prompt_eval_duration,eval_duration}'
   ```

4. Repeat the second `curl` command five times without unloading the model. Treat the
   first sample as cache warm-up and report the remaining range.

## Verification

- The queue probe exits zero and reports all 250 submissions and claims.
- Ollama returns exactly `baseline ok` with `done_reason` equal to `stop`.
- The report includes host, Ollama version, model tag and model ID.

Verified observation: MacBook Pro `Mac15,7`, M3 Pro (12 cores), 36 GB, Ollama
0.23.2, `qwen3.6:latest` ID `07d35212591f`. Cold total/load were 8593.5/8191.7
ms; stable warm totals were 404.3–422.5 ms. Queue submission/claim means were
0.341/0.232 ms for 250 operations.

## Troubleshooting

| Problem | Cause | Fix |
|---|---|---|
| Port 11434 refuses connections | Ollama is not running | Start `ollama serve`. |
| Empty response with `length` | Thinking consumed the output budget | Preserve `/no_think` and `"think":false`. |
| Model tag is missing | Reference model is not installed | Run `ollama pull qwen3.6:latest` and record its ID. |
| Results vary widely | Caches are changing | Separate forced-cold and warm runs and retain every sample. |

## Related Runbooks

- [RUNBOOK-0004](0004-run-tests.md) — Run Tests and Checks
- [RUNBOOK-0020](0020-operate-agent-task-worker.md) — Operate an Agent Task Worker
