//! CLI wiring. Inspection is read-only and independent of model credentials.
use super::*;
use crate::{
    agent_host::{AgentHostBuilder, AgentWorkerOptions},
    agent_model::ModelProviderCatalog,
    agent_resource::ResourceDirectory,
    agent_task_store::{AgentTaskStore, AgentTaskSubmissionResult},
};
use serde_json::Value;

pub async fn run(
    config: Config,
    directories: &[ResourceDirectory],
    tool_directories: &[ResourceDirectory],
    name: &str,
    input: &str,
    json_output: bool,
    non_interactive: bool,
) -> Result<()> {
    let root = std::env::current_dir()?;
    let (catalog, authority) = cli_tool_bindings(&config, &root)?;
    let mut builder = AgentHostBuilder::new(config, &root, ModelProviderCatalog::with_builtins()?)?
        .with_agent_resources(directories.to_vec())
        .with_tool_bindings(tool_directories.to_vec(), catalog, authority);
    let approvals: Arc<dyn ApprovalHandler> = if non_interactive {
        Arc::new(DenyApprovals)
    } else {
        Arc::new(super::terminal::TerminalApprovals)
    };
    builder = builder.with_policy(RuntimePolicy::default(), approvals);
    let host = builder.build(name).await?;
    let (sender, receiver) = watch::channel(false);
    let signal = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sender.send(true);
    });
    let result = host.run(input, receiver).await;
    signal.abort();
    print_run(result?, json_output)
}

fn print_run(run: AgentRun, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        println!(
            "Run: {}\nStatus: {}\nLifecycle: {:?}\nOutcome: {:?}\nReason: {}\nElapsed: {}ms\nBudgets: {}\nUsage: {}",
            run.id,
            run.status,
            run.lifecycle,
            run.outcome,
            run.reason_code.as_deref().unwrap_or("-"),
            run.elapsed_ms,
            serde_json::to_string(&run.budgets.0)?,
            serde_json::to_string(&run.usage.0)?,
        );
        if let Some(output) = &run.output {
            println!("\n{output}");
        }
        if let Some(error) = &run.error {
            println!("Error: {error}");
        }
    }
    ensure!(
        run.outcome == Some(crate::agent_store::RunOutcomeKind::Completed),
        "agent run {} ended with status {}",
        run.id,
        run.status
    );
    Ok(())
}

pub async fn resume(
    config: Config,
    directories: &[ResourceDirectory],
    tool_directories: &[ResourceDirectory],
    id: &str,
    json_output: bool,
    non_interactive: bool,
) -> Result<()> {
    let store = read_store(config.clone())
        .await?
        .context("no runtime history in this workspace")?;
    let previous = store
        .get_run(id)
        .await?
        .context("run not found in workspace")?;
    let root = std::env::current_dir()?;
    let (catalog, authority) = cli_tool_bindings(&config, &root)?;
    let mut builder = AgentHostBuilder::new(config, &root, ModelProviderCatalog::with_builtins()?)?
        .with_agent_resources(directories.to_vec())
        .with_tool_bindings(tool_directories.to_vec(), catalog, authority);
    let approvals: Arc<dyn ApprovalHandler> = if non_interactive {
        Arc::new(DenyApprovals)
    } else {
        Arc::new(super::terminal::TerminalApprovals)
    };
    builder = builder.with_policy(RuntimePolicy::default(), approvals);
    let host = builder.build(&previous.agent_name).await?;
    let (sender, receiver) = watch::channel(false);
    let signal = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sender.send(true);
    });
    let result = host.resume(id, receiver).await;
    signal.abort();
    print_run(result?, json_output)
}

fn cli_tool_bindings(
    config: &Config,
    root: &Path,
) -> Result<(
    tool_binding::ToolImplementationCatalog,
    Arc<HostToolAuthority>,
)> {
    let mut authority = HostToolAuthority::new(root, vec![Capability::ReadOnly])?;
    authority.enroll_path(root)?;
    let sources = config
        .connectors
        .filesystem
        .keys()
        .map(|name| format!("filesystem:{name}"))
        .chain(
            config
                .connectors
                .git
                .keys()
                .map(|name| format!("git:{name}")),
        )
        .chain(config.connectors.s3.keys().map(|name| format!("s3:{name}")))
        .chain(
            config
                .connectors
                .script
                .keys()
                .map(|name| format!("script:{name}")),
        );
    for source in sources {
        authority.enroll_source(source)?;
    }
    Ok((tool_binding::core_catalog()?, Arc::new(authority)))
}

fn cli_host_builder(
    config: Config,
    root: &Path,
    directories: &[ResourceDirectory],
    tool_directories: &[ResourceDirectory],
) -> Result<AgentHostBuilder> {
    let (catalog, authority) = cli_tool_bindings(&config, root)?;
    Ok(
        AgentHostBuilder::new(config, root, ModelProviderCatalog::with_builtins()?)?
            .with_agent_resources(directories.to_vec())
            .with_tool_bindings(tool_directories.to_vec(), catalog, authority)
            .with_policy(RuntimePolicy::default(), Arc::new(DenyApprovals)),
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn enqueue(
    config: Config,
    directories: &[ResourceDirectory],
    tool_directories: &[ResourceDirectory],
    name: &str,
    input: &str,
    request_key: &str,
    queue_limit: u32,
    json_output: bool,
) -> Result<()> {
    let root = std::env::current_dir()?;
    let submitter = cli_host_builder(config, &root, directories, tool_directories)?
        .build_task_submitter(name)
        .await?;
    let result = submitter
        .submit_with_queue_limit(request_key, input, Vec::new(), queue_limit)
        .await?;
    let value = sanitized_json(&result)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        match &result {
            AgentTaskSubmissionResult::Created(task) => {
                println!(
                    "Job: {}\nDisposition: created\nStatus: {:?}",
                    task.id, task.status
                );
            }
            AgentTaskSubmissionResult::ExistingIdentical(task) => {
                println!(
                    "Job: {}\nDisposition: existing_identical\nStatus: {:?}",
                    task.id, task.status
                );
            }
            AgentTaskSubmissionResult::RequestConflict => println!("Disposition: request_conflict"),
            AgentTaskSubmissionResult::QueueFull => println!("Disposition: queue_full"),
        }
    }
    match result {
        AgentTaskSubmissionResult::Created(_) | AgentTaskSubmissionResult::ExistingIdentical(_) => {
            Ok(())
        }
        AgentTaskSubmissionResult::RequestConflict => {
            anyhow::bail!("agent job request key conflicts with an existing submission")
        }
        AgentTaskSubmissionResult::QueueFull => anyhow::bail!("agent job queue is full"),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn worker(
    config: Config,
    directories: &[ResourceDirectory],
    tool_directories: &[ResourceDirectory],
    worker_id: Option<String>,
    max_concurrency: u32,
    lease_seconds: u64,
    heartbeat_seconds: u64,
    poll_ms: u64,
    max_attempts: u32,
    shutdown_seconds: u64,
) -> Result<()> {
    let root = std::env::current_dir()?;
    let options = AgentWorkerOptions {
        worker_id,
        concurrency: max_concurrency as usize,
        lease_duration: Duration::from_secs(lease_seconds),
        heartbeat_interval: Duration::from_secs(heartbeat_seconds),
        polling_interval: Duration::from_millis(poll_ms),
        max_claim_attempts: max_attempts,
        graceful_shutdown_timeout: Duration::from_secs(shutdown_seconds),
    };
    let worker = cli_host_builder(config, &root, directories, tool_directories)?
        .build_worker(options)
        .await?;
    let (sender, receiver) = watch::channel(false);
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = sender.send(true);
        }
    });
    let result = worker.run(receiver).await;
    signal.abort();
    result
}

fn sanitized_json<T: serde::Serialize>(value: &T) -> Result<Value> {
    fn remove_claim_tokens(value: &mut Value) {
        match value {
            Value::Object(object) => {
                object.remove("claim_token");
                for value in object.values_mut() {
                    remove_claim_tokens(value);
                }
            }
            Value::Array(values) => values.iter_mut().for_each(remove_claim_tokens),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(value)?;
    remove_claim_tokens(&mut value);
    Ok(value)
}

async fn read_task_store(config: Config) -> Result<Option<AgentTaskStore>> {
    task_store(config, true).await
}

async fn write_task_store(config: Config) -> Result<Option<AgentTaskStore>> {
    task_store(config, false).await
}

async fn task_store(mut config: Config, read_only: bool) -> Result<Option<AgentTaskStore>> {
    let root = std::env::current_dir()?.canonicalize()?;
    if config.db.path.is_relative() {
        config.db.path = root.join(&config.db.path);
    }
    if !config.db.path.try_exists()? {
        return Ok(None);
    }
    let pool = if read_only {
        tools::read_pool(&config).await?
    } else {
        crate::db::connect(&config).await?
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'agent_tasks')",
    )
    .fetch_one(&pool)
    .await?;
    if !exists {
        return Ok(None);
    }
    Ok(Some(AgentTaskStore::new(pool, &workspace_id(&root)?)?))
}

pub async fn jobs(config: Config, limit: u32, json_output: bool) -> Result<()> {
    let inspections = match read_task_store(config).await? {
        Some(store) => store.list_inspections(limit).await?,
        None => Vec::new(),
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&sanitized_json(&inspections)?)?
        );
    } else {
        println!("{:<36} {:<18} {:<24} AGENT", "JOB", "STATUS", "REASON");
        for inspection in inspections {
            println!(
                "{:<36} {:<18?} {:<24} {}",
                inspection.task.id,
                inspection.task.status,
                inspection.task.scheduling_reason.as_deref().unwrap_or("-"),
                inspection.task.agent_name
            );
        }
    }
    Ok(())
}

pub async fn job_inspect(
    config: Config,
    id: &str,
    after_sequence: i64,
    limit: u32,
    json_output: bool,
) -> Result<()> {
    let store = read_task_store(config)
        .await?
        .context("no durable agent jobs in this workspace")?;
    let inspection = store
        .inspect(id)
        .await?
        .context("agent job not found in this workspace")?;
    let events = store.events(id, after_sequence, limit).await?;
    let cursor = (events.len() == limit as usize)
        .then(|| events.last().map(|event| event.sequence))
        .flatten();
    if json_output {
        let value = json!({
            "task": inspection.task,
            "run": inspection.run,
            "events": events,
            "next_after_sequence": cursor,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&sanitized_json(&value)?)?
        );
    } else {
        let task = inspection.task;
        println!(
            "Job: {}\nAgent: {}\nStatus: {:?}\nReason: {}\nRun: {}",
            task.id,
            task.agent_name,
            task.status,
            task.scheduling_reason.as_deref().unwrap_or("-"),
            task.run_id.as_deref().unwrap_or("-")
        );
        for event in events {
            println!(
                "{:>5}  {}  {}",
                event.sequence, event.event_type, event.payload.0
            );
        }
        if let Some(cursor) = cursor {
            println!("Next page: --after-sequence {cursor}");
        }
    }
    Ok(())
}

pub async fn job_cancel(config: Config, id: &str, json_output: bool) -> Result<()> {
    let store = write_task_store(config)
        .await?
        .context("no durable agent jobs in this workspace")?;
    let task = store.cancel(id).await?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&sanitized_json(&task)?)?);
    } else {
        println!(
            "Job: {}\nStatus: {:?}\nReason: {}",
            task.id,
            task.status,
            task.scheduling_reason.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

async fn read_store(mut config: Config) -> Result<Option<AgentRunStore>> {
    let root = std::env::current_dir()?.canonicalize()?;
    if config.db.path.is_relative() {
        config.db.path = root.join(&config.db.path);
    }
    if !config.db.path.try_exists()? {
        return Ok(None);
    }
    let pool = tools::read_pool(&config).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'agent_runs')",
    )
    .fetch_one(&pool)
    .await?;
    if !exists {
        return Ok(None);
    }
    Ok(Some(AgentRunStore::new(pool, &workspace_id(&root)?)?))
}

pub async fn history(config: Config, limit: u32, json_output: bool) -> Result<()> {
    let runs = match read_store(config).await? {
        Some(store) => store.history(limit).await?,
        None => vec![],
    };
    if json_output {
        println!("{}", serde_json::to_string_pretty(&runs)?);
    } else {
        println!("{:<36} {:<12} {:<24} AGENT", "RUN", "STATUS", "RECOVERY");
        for run in runs {
            println!(
                "{:<36} {:<12} {:<24} {}",
                run.id,
                run.status,
                format!("{:?}", run.recovery_disposition),
                run.agent_name
            );
        }
    }
    Ok(())
}

pub async fn inspect(
    config: Config,
    id: &str,
    after_sequence: i64,
    limit: u32,
    json_output: bool,
) -> Result<()> {
    let store = read_store(config)
        .await?
        .context("no runtime history in this workspace")?;
    let run = store
        .get_run(id)
        .await?
        .context("run not found in this workspace")?;
    let events = store.events(id, after_sequence, limit).await?;
    let artifacts = store.artifacts(id).await?;
    let lineage = store.lineage(id).await?;
    let children = store.children(id).await?;
    let working_state = store.working_state(id).await?;
    let invocations: Vec<Value> = store
        .tool_invocations(id)
        .await?
        .into_iter()
        .map(|call| {
            json!({
                "call_id":call.call_id, "tool":call.tool_name, "status":call.status,
                "error":call.error, "started_at":call.started_at, "completed_at":call.completed_at
            })
        })
        .collect();
    let cursor = if events.len() == limit as usize {
        events.last().map(|e| e.sequence)
    } else {
        None
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"run":run, "working_state":working_state, "events":events, "tool_invocations":invocations, "next_after_sequence":cursor, "artifacts":artifacts, "lineage":lineage, "children":children})
            )?
        );
    } else {
        println!(
            "Run: {}\nAgent: {}\nModel: {}\nWorkspace: {}\nStatus: {}\nLifecycle: {:?}\nOutcome: {:?}\nRecovery: {:?}\nReason: {}\nElapsed: {}ms\nBudgets: {}\nUsage: {}",
            run.id, run.agent_name, run.model, run.workspace_id, run.status,
            run.lifecycle, run.outcome, run.recovery_disposition, run.reason_code.as_deref().unwrap_or("-"),
            run.elapsed_ms, serde_json::to_string(&run.budgets.0)?,
            serde_json::to_string(&run.usage.0)?
        );
        println!(
            "Root: {}\nParent: {:?}\nDepth: {}",
            lineage.root_run_id, lineage.parent_run_id, lineage.depth
        );
        println!(
            "Working state: {:?}\nProjection: {}\nRevision: {}\nEvent cursor: {}",
            working_state.status,
            working_state
                .projection_version
                .map_or("-".into(), |v| v.to_string()),
            working_state.revision.map_or("-".into(), |v| v.to_string()),
            working_state
                .event_cursor
                .map_or("-".into(), |v| v.to_string()),
        );
        if let Some(snapshot) = &working_state.snapshot {
            println!(
                "Latest projected event: {} {}\nArtifacts: {} retained, {} total, {} omitted",
                snapshot.latest_event.sequence,
                snapshot.latest_event.event_type,
                snapshot.artifacts.items.len(),
                snapshot.artifacts.total,
                snapshot.artifacts.omitted,
            );
            for artifact in &snapshot.artifacts.items {
                println!(
                    "Artifact reference: {} {} {} {} bytes",
                    artifact.sequence, artifact.relative_path, artifact.sha256, artifact.size
                );
            }
        }
        for child in children {
            println!("Child: {}", child.run_id);
        }
        for event in events {
            println!(
                "{:>5}  {:>8}ms  {}  {}",
                event.sequence,
                event.timestamp - run.created_at,
                event.event_type,
                event.payload.0
            );
        }
        if let Some(cursor) = cursor {
            println!("Next page: --after-sequence {cursor}");
        }
        for artifact in artifacts {
            println!(
                "Artifact: {} ({} bytes; sha256 {})",
                artifact.relative_path, artifact.size, artifact.sha256
            );
        }
        if let Some(output) = run.output {
            println!("\nOutput:\n{output}");
        }
        if let Some(error) = run.error {
            println!("\nError: {error}");
        }
    }
    Ok(())
}
