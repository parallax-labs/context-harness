# Runbooks

This directory contains operational runbooks for Context Harness. Each runbook
is a self-contained, step-by-step procedure for a discrete task: building,
deploying, maintaining, or troubleshooting the system.

| Layer | Purpose | Authority |
|-------|---------|-----------|
| **PRD** (`docs/prd/`) | What we build and why (user perspective) | Product intent |
| **ADR** (`docs/adr/`) | Why we chose a specific approach | Architectural rationale |
| **Spec** (`docs/spec/`) | Exactly how the system behaves | Behavioral contract |
| **Design** (`docs/design/`) | Exploration, planning, implementation guides | Not authoritative |
| **Runbook** (`docs/runbook/`) | Step-by-step operational procedures | Operational reference |

See [RUNBOOK-0000](0000-runbook-policy.md) for the full policy.

## Index

### Build and Development

| Runbook | Title | Status |
|---------|-------|--------|
| [0001](0001-local-dev-setup.md) | Local Development Setup | Active |
| [0002](0002-build-cli.md) | Build the CLI | Active |
| [0003](0003-build-tauri-app.md) | Build the Tauri Desktop App | Active |
| [0004](0004-run-tests.md) | Run Tests and Checks | Active |

### Release and CI/CD

| Runbook | Title | Status |
|---------|-------|--------|
| [0005](0005-cut-release.md) | Cut a Release | Active |
| [0006](0006-ci-failures.md) | Diagnose CI Failures | Active |

### Deployment

| Runbook | Title | Status |
|---------|-------|--------|
| [0007](0007-deploy-docker.md) | Deploy with Docker | Active |
| [0008](0008-deploy-systemd.md) | Deploy with systemd | Active |
| [0009](0009-deploy-mcp-cursor.md) | Deploy MCP Server for Cursor | Active |

### Data and Content

| Runbook | Title | Status |
|---------|-------|--------|
| [0010](0010-workspace-init.md) | Initialize a Workspace | Active |
| [0011](0011-sync-connectors.md) | Sync Connectors | Active |
| [0012](0012-manage-embeddings.md) | Manage Embeddings | Active |
| [0013](0013-database-maintenance.md) | Database Maintenance | Active |

### Registry and Extensions

| Runbook | Title | Status |
|---------|-------|--------|
| [0014](0014-registry-init-update.md) | Registry Init and Update | Active |
| [0015](0015-author-lua-extension.md) | Author a Lua Extension | Active |

### Documentation Site

| Runbook | Title | Status |
|---------|-------|--------|
| [0016](0016-deploy-docs-site.md) | Deploy Documentation Site | Active |

### Troubleshooting

| Runbook | Title | Status |
|---------|-------|--------|
| [0017](0017-common-errors.md) | Common Errors Reference | Active |

## Creating a New Runbook

1. Copy the template below into a new file named `NNNN-short-title.md` where
   `NNNN` is the next sequential number.
2. Fill in all required sections. See [RUNBOOK-0000](0000-runbook-policy.md)
   for writing guidelines.
3. Add an entry to the appropriate index section above.
4. Verify the runbook end-to-end before marking it Active.

### Template

```markdown
# RUNBOOK-NNNN: Title

**Status:** Draft | Active | Deprecated
**Date:** YYYY-MM-DD
**Author:** ...
**Last Verified:** YYYY-MM-DD

## Purpose

One or two sentences: what this runbook accomplishes and when to use it.

## Prerequisites

- Tool or access requirement
- Environment variable or config
- Prior runbook to complete first (if any)

## Steps

1. First action.

   ```bash
   command --flag value
   ```

   Expected output: description of what you should see.

2. Second action.

   ...

## Verification

How to confirm the procedure succeeded.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| Error message or symptom | Why it happens | What to do |

## Rollback

How to undo the procedure if something goes wrong (required for
destructive operations).
```
