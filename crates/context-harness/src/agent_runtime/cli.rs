//! CLI wiring. Inspection is read-only and independent of model credentials.
use super::*;
use crate::agent_resource::{load_resources, ResourceDirectory};
use serde_json::Value;
use std::collections::BTreeMap;

pub async fn run(
    config: Config,
    directories: &[ResourceDirectory],
    name: &str,
    input: &str,
    json_output: bool,
    non_interactive: bool,
) -> Result<()> {
    let mut resources = load_resources(directories, &config)?;
    let resource = resources
        .remove(name)
        .context("standalone agent not found; legacy agents remain prompt-only")?;
    let alias = &resource.definition.agent.model;
    let definition = config
        .models
        .get(alias)
        .context("model alias not found")?
        .clone();
    let models = ModelRegistry::from_config(&BTreeMap::from([(alias.clone(), definition)]))?;
    let mut runtime = AgentRuntime::new(config, &std::env::current_dir()?, models).await?;
    if !non_interactive {
        runtime = runtime.with_policy(
            RuntimePolicy::default(),
            Arc::new(super::terminal::TerminalApprovals),
        );
    }
    let (sender, receiver) = watch::channel(false);
    let signal = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sender.send(true);
    });
    let result = runtime.run(&resource, input, receiver).await;
    signal.abort();
    print_run(result?, json_output)
}

fn print_run(run: AgentRun, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(&run)?);
    } else {
        println!("Run: {}\nStatus: {}", run.id, run.status);
        if let Some(output) = &run.output {
            println!("\n{output}");
        }
        if let Some(error) = &run.error {
            println!("Error: {error}");
        }
    }
    ensure!(
        run.status == "completed",
        "agent run {} ended with status {}",
        run.id,
        run.status
    );
    Ok(())
}

pub async fn resume(
    config: Config,
    directories: &[ResourceDirectory],
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
    let mut resources = load_resources(directories, &config)?;
    let resource = resources
        .remove(&previous.agent_name)
        .context("agent resource no longer exists")?;
    let alias = &resource.definition.agent.model;
    let definition = config
        .models
        .get(alias)
        .context("model alias not found")?
        .clone();
    let models = ModelRegistry::from_config(&BTreeMap::from([(alias.clone(), definition)]))?;
    let mut runtime = AgentRuntime::new(config, &std::env::current_dir()?, models).await?;
    if !non_interactive {
        runtime = runtime.with_policy(
            RuntimePolicy::default(),
            Arc::new(super::terminal::TerminalApprovals),
        );
    }
    let (sender, receiver) = watch::channel(false);
    let signal = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = sender.send(true);
    });
    let result = runtime.resume(id, &resource, receiver).await;
    signal.abort();
    print_run(result?, json_output)
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
        println!("{:<36} {:<12} AGENT", "RUN", "STATUS");
        for run in runs {
            println!("{:<36} {:<12} {}", run.id, run.status, run.agent_name);
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
                &json!({"run":run, "events":events, "tool_invocations":invocations, "next_after_sequence":cursor, "artifacts":artifacts})
            )?
        );
    } else {
        println!(
            "Run: {}\nAgent: {}\nModel: {}\nWorkspace: {}\nStatus: {}",
            run.id, run.agent_name, run.model, run.workspace_id, run.status
        );
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
