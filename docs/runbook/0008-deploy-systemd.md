# RUNBOOK-0008: Deploy with systemd

**Status:** Active
**Date:** 2026-02-28
**Author:** pjones
**Last Verified:** 2026-02-28

## Purpose

Deploy Context Harness as a systemd service on a Linux server, with cron-based periodic sync and log management.

## Prerequisites

- Linux server with systemd (Debian/Ubuntu, RHEL, etc.)
- Root or sudo access
- The `ctx` binary (built locally or downloaded from a GitHub Release -- see [RUNBOOK-0002](0002-build-cli.md) or [RUNBOOK-0005](0005-cut-release.md))
- A `ctx.toml` configuration file prepared for the target environment

## Steps

1. Create a dedicated user and group.

   ```bash
   sudo useradd --system --create-home --shell /usr/sbin/nologin ctx
   ```

2. Create the directory structure.

   ```bash
   sudo mkdir -p /opt/context-harness/{config,data,connectors,tools,agents}
   sudo chown -R ctx:ctx /opt/context-harness
   ```

3. Copy the binary.

   ```bash
   sudo cp target/release/ctx /opt/context-harness/ctx
   sudo chmod +x /opt/context-harness/ctx
   ```

4. Copy your configuration.

   ```bash
   sudo cp config/ctx.toml /opt/context-harness/config/ctx.toml
   sudo chown ctx:ctx /opt/context-harness/config/ctx.toml
   ```

   Ensure the config uses absolute paths appropriate for the server:

   ```toml
   [db]
   path = "/opt/context-harness/data/ctx.sqlite"

   [server]
   bind = "127.0.0.1:7331"

   [connectors.filesystem.docs]
   root = "/opt/context-harness/content"
   include_globs = ["**/*.md", "**/*.txt"]
   ```

5. Initialize the database.

   ```bash
   sudo -u ctx /opt/context-harness/ctx init --config /opt/context-harness/config/ctx.toml
   ```

   Expected: database created at the configured path.

6. Run an initial sync.

   ```bash
   sudo -u ctx /opt/context-harness/ctx sync all --full --config /opt/context-harness/config/ctx.toml
   ```

7. Create the systemd unit file at `/etc/systemd/system/context-harness.service`:

   ```ini
   [Unit]
   Description=Context Harness MCP Server
   After=network.target

   [Service]
   Type=simple
   User=ctx
   Group=ctx
   WorkingDirectory=/opt/context-harness
   ExecStart=/opt/context-harness/ctx serve mcp --config /opt/context-harness/config/ctx.toml
   Environment=OPENAI_API_KEY=sk-...
   Restart=on-failure
   RestartSec=5

   [Install]
   WantedBy=multi-user.target
   ```

8. Enable and start the service.

   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable context-harness
   sudo systemctl start context-harness
   ```

9. Set up periodic sync via cron.

   ```bash
   sudo crontab -u ctx -e
   ```

   Add:

   ```
   */15 * * * * /opt/context-harness/ctx sync all --config /opt/context-harness/config/ctx.toml >> /opt/context-harness/data/sync.log 2>&1
   ```

## Verification

1. Check service status:

   ```bash
   sudo systemctl status context-harness
   ```

   Expected: `Active: active (running)`.

2. Test the health endpoint:

   ```bash
   curl -s http://127.0.0.1:7331/health
   ```

   Expected: `200 OK` response.

3. Test the MCP endpoint:

   ```bash
   curl -s http://127.0.0.1:7331/mcp -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
   ```

   Expected: JSON response listing available tools.

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| Service fails to start | Config path wrong or permissions | Check `journalctl -u context-harness -e` for error details |
| "Address already in use" | Another process on port 7331 | Change `[server].bind` port or stop the other process |
| Cron sync not running | Crontab not saved or user wrong | Verify with `sudo crontab -u ctx -l` |
| Permission denied on database | Database owned by wrong user | `sudo chown ctx:ctx /opt/context-harness/data/ctx.sqlite*` |

## Rollback

```bash
sudo systemctl stop context-harness
sudo systemctl disable context-harness
sudo rm /etc/systemd/system/context-harness.service
sudo systemctl daemon-reload
```

## Related Runbooks

- [RUNBOOK-0002](0002-build-cli.md) -- Build the binary
- [RUNBOOK-0005](0005-cut-release.md) -- Download a release binary
- [RUNBOOK-0010](0010-workspace-init.md) -- Configure the workspace
