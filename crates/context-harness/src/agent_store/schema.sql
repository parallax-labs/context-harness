-- Runtime history shares the canonical context database. Workspace IDs are
-- logical bindings, not foreign keys: the workspace registry lives in TOML.
CREATE TABLE IF NOT EXISTS agent_runs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    agent_version TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'cancelled')),
    lifecycle TEXT NOT NULL DEFAULT 'active' CHECK (lifecycle IN ('active', 'suspended', 'terminal')),
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('completed', 'blocked', 'needs_user_input', 'failed', 'limit_exceeded', 'cancelled')),
    reason_code TEXT,
    reason_detail TEXT CHECK (reason_detail IS NULL OR json_valid(reason_detail)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    input TEXT NOT NULL,
    output TEXT,
    error TEXT,
    last_sequence INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence >= 0),
    budgets TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(budgets)),
    usage TEXT NOT NULL DEFAULT '{"model_turns":0,"input_tokens":null,"output_tokens":null,"total_tokens":null,"responses_with_usage":0,"responses_without_usage":0,"tool_calls":0,"token_accounting":"unavailable"}' CHECK (json_valid(usage))
);
CREATE INDEX IF NOT EXISTS idx_agent_runs_workspace
    ON agent_runs(workspace_id, created_at DESC, id);
CREATE TABLE IF NOT EXISTS agent_run_working_state (
    run_id TEXT PRIMARY KEY REFERENCES agent_runs(id),
    projection_version INTEGER NOT NULL CHECK (projection_version > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    event_cursor INTEGER NOT NULL CHECK (event_cursor >= 0),
    updated_at INTEGER NOT NULL,
    snapshot TEXT NOT NULL CHECK (json_valid(snapshot))
);
CREATE TABLE IF NOT EXISTS agent_events (
    run_id TEXT NOT NULL REFERENCES agent_runs(id),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    timestamp INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    PRIMARY KEY (run_id, sequence)
);
CREATE TABLE IF NOT EXISTS tool_invocations (
    run_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    requested_sequence INTEGER NOT NULL,
    tool_name TEXT NOT NULL,
    arguments TEXT NOT NULL CHECK (json_valid(arguments)),
    status TEXT NOT NULL CHECK (status IN ('requested', 'started', 'completed', 'failed', 'denied')),
    result TEXT CHECK (result IS NULL OR json_valid(result)),
    error TEXT,
    started_at INTEGER,
    completed_at INTEGER,
    PRIMARY KEY (run_id, call_id),
    FOREIGN KEY (run_id, requested_sequence) REFERENCES agent_events(run_id, sequence)
);
CREATE TABLE IF NOT EXISTS agent_checkpoints (
    run_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    turn INTEGER NOT NULL CHECK (turn >= 0),
    created_at INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (json_valid(state)),
    PRIMARY KEY (run_id, sequence),
    FOREIGN KEY (run_id, sequence) REFERENCES agent_events(run_id, sequence)
);

CREATE TABLE IF NOT EXISTS agent_run_lineage (
    run_id TEXT PRIMARY KEY REFERENCES agent_runs(id),
    parent_run_id TEXT REFERENCES agent_runs(id),
    root_run_id TEXT NOT NULL REFERENCES agent_runs(id),
    depth INTEGER NOT NULL CHECK (depth BETWEEN 0 AND 4),
    parent_call_id TEXT,
    UNIQUE (parent_run_id, parent_call_id),
    FOREIGN KEY (parent_run_id, parent_call_id) REFERENCES tool_invocations(run_id, call_id),
    CHECK ((depth = 0 AND parent_run_id IS NULL AND parent_call_id IS NULL AND root_run_id = run_id)
        OR (depth > 0 AND parent_run_id IS NOT NULL AND parent_call_id IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS idx_agent_run_lineage_root ON agent_run_lineage(root_run_id);

-- Accepted background intent is separate from execution history. Worker-owned
-- fields are present for forward-compatible Phase 4B migrations but Phase 4A
-- writes only queued and queued-cancelled tasks.
CREATE TABLE IF NOT EXISTS agent_tasks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    request_key TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    agent_version TEXT NOT NULL,
    accepted_identity TEXT NOT NULL CHECK (json_valid(accepted_identity)),
    accepted_identity_digest TEXT NOT NULL,
    input TEXT NOT NULL,
    payload_identity BLOB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'claimed', 'cancel_requested', 'terminal')),
    scheduling_reason TEXT,
    scheduling_detail TEXT CHECK (scheduling_detail IS NULL OR json_valid(scheduling_detail)),
    claim_owner TEXT,
    claim_token TEXT,
    lease_expires_at INTEGER,
    claim_attempts INTEGER NOT NULL DEFAULT 0 CHECK (claim_attempts >= 0),
    run_id TEXT REFERENCES agent_runs(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    claimed_at INTEGER,
    completed_at INTEGER,
    last_sequence INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence >= 0),
    UNIQUE (workspace_id, request_key),
    CHECK ((status IN ('queued', 'terminal') AND claim_owner IS NULL AND claim_token IS NULL AND lease_expires_at IS NULL)
        OR (status IN ('claimed', 'cancel_requested') AND claim_owner IS NOT NULL AND claim_token IS NOT NULL AND lease_expires_at IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_workspace_queue
    ON agent_tasks(workspace_id, status, created_at, id);
CREATE TABLE IF NOT EXISTS agent_task_events (
    task_id TEXT NOT NULL REFERENCES agent_tasks(id),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    timestamp INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    PRIMARY KEY (task_id, sequence)
);
