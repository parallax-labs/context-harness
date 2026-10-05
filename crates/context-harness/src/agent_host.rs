//! Public assembly seam for trusted hosts that execute standalone agents.
//!
//! Builder configuration is inert. [`AgentHostBuilder::build`] initializes the
//! existing runtime store and binds explicitly supplied implementations, but it
//! does not call a model, execute a tool, resolve credentials, or start workers.

use crate::{
    agent_model::{ModelProviderCatalog, ModelRegistry},
    agent_resource::{load_resources, LoadedAgentResource, ResourceDirectory},
    agent_runtime::{
        files::RunOwnershipUnavailable,
        policy::{ApprovalHandler, DenyApprovals, RuntimePolicy},
        run_budgets, static_tool_identity, workspace_id, AgentRuntime,
    },
    agent_store::{AgentRun, AgentRunStore, RecoveryDisposition},
    agent_task_store::{
        AcceptedTaskIdentity, AgentTaskClaim, AgentTaskStatus, AgentTaskStore, AgentTaskSubmission,
        AgentTaskSubmissionResult, DEFAULT_QUEUE_LIMIT,
    },
    app_store::SqliteAppStore,
    config::Config,
    tool_binding::{self, HostToolAuthority, ToolImplementationCatalog, CATALOG_CONTRACT_VERSION},
};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path, path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinSet};
use uuid::Uuid;

#[derive(Clone)]
struct ToolBindings {
    directories: Vec<ResourceDirectory>,
    catalog: ToolImplementationCatalog,
    authority: Arc<HostToolAuthority>,
}

/// Trusted inputs for assembling one direct-execution agent host.
#[derive(Clone)]
pub struct AgentHostBuilder {
    config: Config,
    root: PathBuf,
    models: ModelProviderCatalog,
    agent_directories: Vec<ResourceDirectory>,
    tool_bindings: Option<ToolBindings>,
    policy: RuntimePolicy,
    approvals: Arc<dyn ApprovalHandler>,
}

impl AgentHostBuilder {
    pub fn new(config: Config, root: &Path, models: ModelProviderCatalog) -> Result<Self> {
        Ok(Self {
            config,
            root: root
                .canonicalize()
                .context("canonicalizing agent-host workspace root")?,
            models,
            agent_directories: Vec::new(),
            tool_bindings: None,
            policy: RuntimePolicy {
                allow: Vec::new(),
                require_approval: Vec::new(),
            },
            approvals: Arc::new(DenyApprovals),
        })
    }

    pub fn with_agent_resources(mut self, directories: Vec<ResourceDirectory>) -> Self {
        self.agent_directories = directories;
        self
    }

    /// Install trusted tool factories and explicit host-issued authority.
    pub fn with_tool_bindings(
        mut self,
        directories: Vec<ResourceDirectory>,
        catalog: ToolImplementationCatalog,
        authority: Arc<HostToolAuthority>,
    ) -> Self {
        self.tool_bindings = Some(ToolBindings {
            directories,
            catalog,
            authority,
        });
        self
    }

    /// Set the host permission ceiling and approval handler.
    ///
    /// The builder grants no capabilities unless this method is called.
    pub fn with_policy(
        mut self,
        policy: RuntimePolicy,
        approvals: Arc<dyn ApprovalHandler>,
    ) -> Self {
        self.policy = policy;
        self.approvals = approvals;
        self
    }

    /// Assemble the existing direct runtime for one agent and its delegation graph.
    pub async fn build(self, agent_name: &str) -> Result<AgentHost> {
        self.build_with_initialized_store(agent_name, false).await
    }

    async fn build_initialized(self, agent_name: &str) -> Result<AgentHost> {
        self.build_with_initialized_store(agent_name, true).await
    }

    async fn build_with_initialized_store(
        self,
        agent_name: &str,
        initialized: bool,
    ) -> Result<AgentHost> {
        ensure!(!agent_name.trim().is_empty(), "agent name is required");
        let resources = load_resources(&self.agent_directories, &self.config)?;
        let resource = resources
            .get(agent_name)
            .cloned()
            .context("standalone agent not found; profiles are prompt-only")?;
        let definitions = reachable_model_definitions(&self.config, &resources, agent_name)?;
        let models = ModelRegistry::from_config_with_catalog(&definitions, &self.models)?;
        let runtime = if initialized {
            AgentRuntime::new_initialized(self.config, &self.root, models).await?
        } else {
            AgentRuntime::new(self.config, &self.root, models).await?
        };
        let mut runtime = runtime
            .with_resources(resources)
            .with_policy(self.policy, self.approvals);
        if let Some(bindings) = self.tool_bindings {
            runtime = runtime
                .with_tool_binding_catalog(
                    &bindings.directories,
                    &bindings.catalog,
                    bindings.authority,
                )
                .await?;
        }
        Ok(AgentHost { runtime, resource })
    }

    /// Resolve and persist accepted task intent without constructing providers,
    /// binding tools, resolving secrets, or invoking runtime behavior.
    pub async fn build_task_submitter(self, agent_name: &str) -> Result<AgentTaskSubmitter> {
        let resolved = self.resolve_task_identity(agent_name)?;
        let mut config = self.config;
        if config.db.path.is_relative() {
            config.db.path = self.root.join(&config.db.path);
        }
        SqliteAppStore::initialize_config(&config).await?;
        let app = SqliteAppStore::connect(&config).await?;
        let store = app.agent_tasks(&resolved.workspace_id)?;
        Ok(AgentTaskSubmitter {
            store,
            agent_name: resolved.agent_name,
            agent_version: resolved.agent_version,
            accepted_identity: resolved.accepted_identity,
        })
    }

    /// Assemble an explicitly started foreground worker. No background work is
    /// started until [`AgentWorker::run`] is awaited.
    pub async fn build_worker(self, options: AgentWorkerOptions) -> Result<AgentWorker> {
        options.validate()?;
        let mut config = self.config.clone();
        if config.db.path.is_relative() {
            config.db.path = self.root.join(&config.db.path);
        }
        SqliteAppStore::initialize_config(&config).await?;
        let app = SqliteAppStore::connect(&config).await?;
        let store = app.agent_tasks(&workspace_id(&self.root)?)?;
        Ok(AgentWorker {
            builder: self,
            store,
            options,
        })
    }

    /// Compute the immutable, non-secret identity accepted by a task. This
    /// static path does not open the application database.
    pub fn resolve_task_identity(&self, agent_name: &str) -> Result<ResolvedTaskIdentity> {
        ensure!(!agent_name.trim().is_empty(), "agent name is required");
        let resources = load_resources(&self.agent_directories, &self.config)?;
        let resource = resources
            .get(agent_name)
            .context("standalone agent not found; profiles are prompt-only")?;
        let reachable = reachable_agent_resources(&resources, agent_name)?;
        let definitions = reachable_model_definitions(&self.config, &resources, agent_name)?;
        self.models.validate_config(&definitions)?;

        let mut models = BTreeMap::new();
        for (alias, definition) in &definitions {
            let implementation = self.models.implementation(&definition.provider)?;
            models.insert(
                alias.clone(),
                serde_json::json!({
                    "provider": definition.provider,
                    "model": definition.model,
                    "implementation_id": implementation.id(),
                    "implementation_version": implementation.version(),
                }),
            );
        }

        let mut resolved_bindings = BTreeMap::new();
        if let Some(bindings) = &self.tool_bindings {
            let selected_names = reachable
                .values()
                .flat_map(|resource| resource.definition.agent.tools.iter())
                .collect::<std::collections::HashSet<_>>();
            let loaded = tool_binding::load_resources(&bindings.directories, &self.config)?
                .into_iter()
                .filter(|(name, _)| selected_names.contains(name))
                .collect();
            resolved_bindings = tool_binding::resolve_resources(loaded, &bindings.catalog)?;
        }
        let tools = static_tool_identity(
            &self.config,
            &self.root,
            &reachable,
            &resolved_bindings,
            &self.policy,
        )?;
        let agent_versions = reachable
            .iter()
            .map(|(name, resource)| (name.clone(), resource.version.clone()))
            .collect::<BTreeMap<_, _>>();
        let workspace_id = workspace_id(&self.root)?;
        let tool_authority = if let Some(bindings) = &self.tool_bindings {
            let mut paths = bindings
                .authority
                .paths()
                .iter()
                .map(|path| {
                    path.strip_prefix(&self.root)
                        .unwrap_or(path)
                        .as_os_str()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>();
            paths.sort();
            let mut sources = bindings.authority.sources().to_vec();
            sources.sort();
            Some(serde_json::json!({
                "capabilities": sorted_json_values(bindings.authority.capabilities())?,
                "paths": paths,
                "sources": sources,
            }))
        } else {
            None
        };
        let accepted_identity = AcceptedTaskIdentity::new(serde_json::json!({
            "schema_version": 1,
            "workspace_id": workspace_id,
            "root_agent": {"name": agent_name, "version": resource.version},
            "reachable_agents": agent_versions,
            "models": models,
            "tools": tools,
            "binding_contract_version": CATALOG_CONTRACT_VERSION,
            "host_policy": {
                "allow": sorted_json_values(&self.policy.allow)?,
                "require_approval": sorted_json_values(&self.policy.require_approval)?,
            },
            "tool_authority": tool_authority,
        }))?;
        Ok(ResolvedTaskIdentity {
            workspace_id,
            agent_name: agent_name.to_owned(),
            agent_version: resource.version.clone(),
            accepted_identity,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentWorkerOptions {
    pub worker_id: Option<String>,
    pub concurrency: usize,
    pub lease_duration: Duration,
    pub heartbeat_interval: Duration,
    pub polling_interval: Duration,
    pub max_claim_attempts: u32,
    pub graceful_shutdown_timeout: Duration,
}

impl Default for AgentWorkerOptions {
    fn default() -> Self {
        Self {
            worker_id: None,
            concurrency: 1,
            lease_duration: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(20),
            polling_interval: Duration::from_millis(500),
            max_claim_attempts: 3,
            graceful_shutdown_timeout: Duration::from_secs(30),
        }
    }
}

impl AgentWorkerOptions {
    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=32).contains(&self.concurrency),
            "worker concurrency must be 1-32"
        );
        ensure!(
            (5..=3_600).contains(&self.lease_duration.as_secs()),
            "lease duration must be 5-3600 seconds"
        );
        ensure!(
            self.heartbeat_interval >= Duration::from_secs(1)
                && self.heartbeat_interval < self.lease_duration / 2,
            "heartbeat interval must be at least one second and less than half the lease"
        );
        ensure!(
            self.polling_interval >= Duration::from_millis(50)
                && self.polling_interval <= Duration::from_secs(60),
            "polling interval must be 50ms-60s"
        );
        ensure!(
            (1..=100).contains(&self.max_claim_attempts),
            "maximum claim attempts must be 1-100"
        );
        ensure!(
            self.graceful_shutdown_timeout >= Duration::from_secs(1)
                && self.graceful_shutdown_timeout <= Duration::from_secs(3_600),
            "graceful shutdown timeout must be 1-3600 seconds"
        );
        if let Some(worker_id) = &self.worker_id {
            ensure!(
                !worker_id.is_empty()
                    && worker_id.len() <= 128
                    && !worker_id.chars().any(char::is_control),
                "worker ID must contain 1-128 non-control UTF-8 bytes"
            );
        }
        Ok(())
    }
}

/// Explicit foreground executor for durably accepted agent tasks.
pub struct AgentWorker {
    builder: AgentHostBuilder,
    store: AgentTaskStore,
    options: AgentWorkerOptions,
}

enum WorkerClaim {
    New(AgentTaskClaim),
    Reconcile(AgentTaskClaim),
}

impl AgentWorker {
    pub fn store(&self) -> &AgentTaskStore {
        &self.store
    }

    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let worker_id = self
            .options
            .worker_id
            .clone()
            .unwrap_or_else(|| format!("worker-{}", Uuid::new_v4()));
        let mut active = JoinSet::new();
        loop {
            while !*shutdown.borrow() && active.len() < self.options.concurrency {
                let claim = if let Some(expired) = self
                    .store
                    .claim_next_expired(&worker_id, self.options.lease_duration.as_secs())
                    .await?
                {
                    Some(WorkerClaim::Reconcile(expired.claim))
                } else {
                    self.store
                        .claim_next(
                            &worker_id,
                            self.options.lease_duration.as_secs(),
                            self.options.max_claim_attempts,
                        )
                        .await?
                        .map(WorkerClaim::New)
                };
                let Some(claim) = claim else { break };
                let builder = self.builder.clone();
                let store = self.store.clone();
                let options = self.options.clone();
                let task_shutdown = shutdown.clone();
                active.spawn(async move {
                    process_claim(builder, store, options, claim, task_shutdown).await
                });
            }

            if *shutdown.borrow() {
                break;
            }
            if active.is_empty() {
                tokio::select! {
                    changed = shutdown.changed() => { changed.context("worker shutdown channel closed")?; }
                    () = tokio::time::sleep(self.options.polling_interval) => {}
                }
            } else {
                tokio::select! {
                    changed = shutdown.changed() => { changed.context("worker shutdown channel closed")?; }
                    joined = active.join_next() => {
                        joined.context("worker task set ended unexpectedly")???;
                    }
                    () = tokio::time::sleep(self.options.polling_interval) => {}
                }
            }
        }

        let deadline = tokio::time::Instant::now() + self.options.graceful_shutdown_timeout;
        while !active.is_empty() {
            match tokio::time::timeout_at(deadline, active.join_next()).await {
                Ok(Some(joined)) => joined??,
                Ok(None) => break,
                Err(_) => {
                    active.abort_all();
                    break;
                }
            }
        }
        Ok(())
    }
}

async fn process_claim(
    builder: AgentHostBuilder,
    store: AgentTaskStore,
    options: AgentWorkerOptions,
    claim: WorkerClaim,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let (claim, reconcile) = match claim {
        WorkerClaim::New(claim) => (claim, false),
        WorkerClaim::Reconcile(claim) => (claim, true),
    };
    if reconcile {
        return reconcile_claim(builder, store, options, claim, shutdown).await;
    }
    if store.get(&claim.task.id).await?.is_some_and(|task| {
        task.status == AgentTaskStatus::CancelRequested && task.run_id.is_none()
    }) {
        store.finish_cancelled_before_run(&claim).await?;
        return Ok(());
    }
    let resolved = match builder.resolve_task_identity(&claim.task.agent_name) {
        Ok(resolved) => resolved,
        Err(_) => {
            store.finish_identity_changed(&claim).await?;
            return Ok(());
        }
    };
    if resolved.accepted_identity().digest() != claim.task.accepted_identity_digest {
        store.finish_identity_changed(&claim).await?;
        return Ok(());
    }
    let resources = load_resources(&builder.agent_directories, &builder.config)?;
    let resource = resources
        .get(&claim.task.agent_name)
        .cloned()
        .context("claimed agent resource disappeared")?;
    if store.get(&claim.task.id).await?.is_some_and(|task| {
        task.status == AgentTaskStatus::CancelRequested && task.run_id.is_none()
    }) {
        store.finish_cancelled_before_run(&claim).await?;
        return Ok(());
    }
    let run = match store
        .link_run(
            &claim,
            &resource.version,
            &resource.definition.agent.model,
            run_budgets(&resource),
        )
        .await
    {
        Ok(run) => run,
        Err(error) => {
            if store.get(&claim.task.id).await?.is_some_and(|task| {
                task.status == AgentTaskStatus::CancelRequested && task.run_id.is_none()
            }) {
                store.finish_cancelled_before_run(&claim).await?;
                return Ok(());
            }
            return Err(error);
        }
    };

    let host = match builder.build_initialized(&claim.task.agent_name).await {
        Ok(host) => host,
        Err(_) => {
            store.fail_linked_run_setup(&claim, &run.id).await?;
            return Ok(());
        }
    };
    let (cancel_tx, cancel_rx) = watch::channel(false);
    if *shutdown.borrow() {
        let _ = cancel_tx.send(true);
    }
    let execution = host.execute_linked(&run, cancel_rx);
    monitor_owned_execution(
        &store,
        &options,
        &claim,
        cancel_tx,
        execution,
        &mut shutdown,
    )
    .await
}

async fn reconcile_claim(
    builder: AgentHostBuilder,
    store: AgentTaskStore,
    options: AgentWorkerOptions,
    claim: AgentTaskClaim,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let workspace_root = builder.root.clone();
    let current = store
        .get(&claim.task.id)
        .await?
        .context("reconciliation task disappeared")?;
    let Some(run_id) = current.run_id.as_deref() else {
        if current.status == AgentTaskStatus::CancelRequested {
            store.finish_cancelled_before_run(&claim).await?;
        } else if current.claim_attempts >= i64::from(options.max_claim_attempts) {
            store.finish_claim_attempts_exhausted(&claim).await?;
        } else {
            store.requeue(&claim).await?;
        }
        return Ok(());
    };

    let runs = store.run_store()?;
    match runs.recovery_disposition(run_id).await? {
        RecoveryDisposition::Complete | RecoveryDisposition::Suspended => {
            store.finish_run_stopped(&claim).await?;
            return Ok(());
        }
        RecoveryDisposition::RestartRequired => {
            store.finish_restart_required(&claim).await?;
            return Ok(());
        }
        RecoveryDisposition::ReconciliationRequired => {
            store.finish_reconciliation_required(&claim).await?;
            return Ok(());
        }
        RecoveryDisposition::ResumeEligible => {}
    }

    let host = match builder.build_initialized(&current.agent_name).await {
        Ok(host) => host,
        Err(_) => {
            return finish_rejected_recovery(&store, &claim, &runs, run_id).await;
        }
    };
    let (cancel_tx, cancel_rx) = watch::channel(current.status == AgentTaskStatus::CancelRequested);
    let execution = host.resume(run_id, cancel_rx);
    tokio::pin!(execution);
    let result = monitor_owned_execution_inner(
        &store,
        &options,
        &claim,
        cancel_tx,
        &mut execution,
        &mut shutdown,
    )
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.downcast_ref::<RunOwnershipUnavailable>().is_some() => {
            observe_owned_run(
                &store,
                &options,
                &claim,
                &runs,
                &workspace_root,
                run_id,
                &mut shutdown,
            )
            .await
        }
        Err(_) => finish_rejected_recovery(&store, &claim, &runs, run_id).await,
    }
}

async fn finish_rejected_recovery(
    store: &AgentTaskStore,
    claim: &AgentTaskClaim,
    runs: &AgentRunStore,
    run_id: &str,
) -> Result<()> {
    if runs.recovery_disposition(run_id).await? == RecoveryDisposition::ReconciliationRequired {
        store.finish_reconciliation_required(claim).await?;
    } else {
        store.finish_restart_required(claim).await?;
    }
    Ok(())
}

async fn observe_owned_run(
    store: &AgentTaskStore,
    options: &AgentWorkerOptions,
    claim: &AgentTaskClaim,
    runs: &AgentRunStore,
    workspace_root: &Path,
    run_id: &str,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()> {
    let mut heartbeat = tokio::time::interval(options.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                store.heartbeat(claim, options.lease_duration.as_secs()).await?;
                match runs.recovery_disposition(run_id).await? {
                    RecoveryDisposition::Complete | RecoveryDisposition::Suspended => {
                        store.finish_run_stopped(claim).await?;
                        return Ok(());
                    }
                    RecoveryDisposition::RestartRequired => {
                        store.finish_restart_required(claim).await?;
                        return Ok(());
                    }
                    RecoveryDisposition::ReconciliationRequired => {
                        store.finish_reconciliation_required(claim).await?;
                        return Ok(());
                    }
                    RecoveryDisposition::ResumeEligible => {
                        match crate::agent_runtime::files::acquire(workspace_root, run_id) {
                            Ok(ownership) => {
                                drop(ownership);
                                return Ok(());
                            }
                            Err(error) if error.downcast_ref::<RunOwnershipUnavailable>().is_some() => {}
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

async fn monitor_owned_execution<F>(
    store: &AgentTaskStore,
    options: &AgentWorkerOptions,
    claim: &AgentTaskClaim,
    cancel_tx: watch::Sender<bool>,
    execution: F,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()>
where
    F: std::future::Future<Output = Result<AgentRun>>,
{
    tokio::pin!(execution);
    monitor_owned_execution_inner(store, options, claim, cancel_tx, &mut execution, shutdown).await
}

async fn monitor_owned_execution_inner<F>(
    store: &AgentTaskStore,
    options: &AgentWorkerOptions,
    claim: &AgentTaskClaim,
    cancel_tx: watch::Sender<bool>,
    execution: &mut std::pin::Pin<&mut F>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()>
where
    F: std::future::Future<Output = Result<AgentRun>>,
{
    let mut heartbeat = tokio::time::interval(options.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut cancellation = tokio::time::interval(options.polling_interval);
    cancellation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    cancellation.tick().await;
    loop {
        tokio::select! {
            result = execution.as_mut() => {
                result?;
                store.finish_run_stopped(claim).await?;
                return Ok(());
            }
            _ = heartbeat.tick() => {
                let task = store.heartbeat(claim, options.lease_duration.as_secs()).await?;
                if task.status == AgentTaskStatus::CancelRequested {
                    let _ = cancel_tx.send(true);
                }
            }
            _ = cancellation.tick() => {
                if store.get(&claim.task.id).await?.is_some_and(|task| task.status == AgentTaskStatus::CancelRequested) {
                    let _ = cancel_tx.send(true);
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    let _ = cancel_tx.send(true);
                }
            }
        }
    }
}

fn sorted_json_values<T: Serialize>(values: &[T]) -> Result<Vec<serde_json::Value>> {
    let mut values = values
        .iter()
        .map(serde_json::to_value)
        .collect::<serde_json::Result<Vec<_>>>()?;
    values.sort_by_key(|value| value.as_str().unwrap_or_default().to_owned());
    Ok(values)
}

pub struct ResolvedTaskIdentity {
    workspace_id: String,
    agent_name: String,
    agent_version: String,
    accepted_identity: AcceptedTaskIdentity,
}

impl ResolvedTaskIdentity {
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }
    pub fn agent_version(&self) -> &str {
        &self.agent_version
    }
    pub fn accepted_identity(&self) -> &AcceptedTaskIdentity {
        &self.accepted_identity
    }
}

/// Public, trusted submission surface for one statically resolved agent.
pub struct AgentTaskSubmitter {
    store: AgentTaskStore,
    agent_name: String,
    agent_version: String,
    accepted_identity: AcceptedTaskIdentity,
}

impl AgentTaskSubmitter {
    pub fn store(&self) -> &AgentTaskStore {
        &self.store
    }
    pub fn accepted_identity(&self) -> &AcceptedTaskIdentity {
        &self.accepted_identity
    }

    pub async fn submit(
        &self,
        request_key: impl Into<String>,
        input: impl Into<String>,
        payload_identity: Vec<u8>,
    ) -> Result<AgentTaskSubmissionResult> {
        self.submit_with_queue_limit(request_key, input, payload_identity, DEFAULT_QUEUE_LIMIT)
            .await
    }

    pub async fn submit_with_queue_limit(
        &self,
        request_key: impl Into<String>,
        input: impl Into<String>,
        payload_identity: Vec<u8>,
        queue_limit: u32,
    ) -> Result<AgentTaskSubmissionResult> {
        self.store
            .submit(AgentTaskSubmission {
                request_key: request_key.into(),
                agent_name: self.agent_name.clone(),
                agent_version: self.agent_version.clone(),
                input: input.into(),
                payload_identity,
                accepted_identity: self.accepted_identity.clone(),
                queue_limit,
            })
            .await
    }
}

/// A configured direct-execution host for one standalone agent.
pub struct AgentHost {
    runtime: AgentRuntime,
    resource: LoadedAgentResource,
}

impl AgentHost {
    pub fn resource(&self) -> &LoadedAgentResource {
        &self.resource
    }

    pub fn store(&self) -> &AgentRunStore {
        self.runtime.store()
    }

    pub async fn run(&self, input: &str, cancel: watch::Receiver<bool>) -> Result<AgentRun> {
        self.runtime.run(&self.resource, input, cancel).await
    }

    pub async fn resume(&self, id: &str, cancel: watch::Receiver<bool>) -> Result<AgentRun> {
        self.runtime.resume(id, &self.resource, cancel).await
    }

    async fn execute_linked(
        &self,
        run: &AgentRun,
        cancel: watch::Receiver<bool>,
    ) -> Result<AgentRun> {
        self.runtime
            .execute_existing_run(run, &self.resource, cancel)
            .await
    }
}

fn reachable_model_definitions(
    config: &Config,
    resources: &BTreeMap<String, LoadedAgentResource>,
    root: &str,
) -> Result<BTreeMap<String, crate::agent_resource::ModelDefinition>> {
    reachable_agent_resources(resources, root)?
        .into_iter()
        .map(|(name, resource)| {
            let alias = resource.definition.agent.model;
            let definition = config
                .models
                .get(&alias)
                .with_context(|| format!("model alias not found for agent '{name}'"))?
                .clone();
            Ok((alias, definition))
        })
        .collect()
}

fn reachable_agent_resources(
    resources: &BTreeMap<String, LoadedAgentResource>,
    root: &str,
) -> Result<BTreeMap<String, LoadedAgentResource>> {
    let mut names = vec![root.to_owned()];
    let mut visited = std::collections::HashSet::new();
    let mut reachable = BTreeMap::new();
    while let Some(name) = names.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        ensure!(
            visited.len() <= 256,
            "delegation catalog exceeds 256 reachable agents"
        );
        let resource = resources
            .get(&name)
            .context("delegation target not found")?;
        names.extend(resource.definition.agent.delegation.allow.iter().cloned());
        reachable.insert(name, resource.clone());
    }
    Ok(reachable)
}
