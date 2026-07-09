//! Multi-workspace routing for the MCP/REST server (SPEC-0014, DESIGN-0008).
//!
//! A [`WorkspaceRouter`] owns a set of [`WorkspaceRuntime`]s (one per registered
//! workspace) and dispatches built-in tool calls to the runtime selected by a
//! request. The router is a *dispatch layer only*: it never merges stores, and
//! the underlying query functions (`search_documents` / `get_document` /
//! `get_sources`) keep operating on a single `&Config`.
//!
//! Multi-workspace behavior is **additive and opt-in**. The default server runs
//! in [`ServerMode::Compat`] — represented internally as a router holding one
//! workspace — and behaves byte-for-byte like the pre-router server. Multi mode
//! ([`ServerMode::Multi`]) is activated only by the explicit `--workspaces`
//! flag and is the only mode that emits workspace-labeled responses.
//!
//! Phase 1 (this module) routes built-in tools to a selected workspace. The
//! `workspace = "all"` fan-out (Phase 2) and origin-scoped extensions (Phase 3)
//! are deferred — `all` currently returns `unsupported_workspace_selector`.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::config::Config;

/// The reserved selector meaning "every enabled workspace".
pub const ALL_SELECTOR: &str = "all";

/// Default per-workspace deadline for an `all` fan-out search (SPEC-0014 R33).
pub const DEFAULT_SEARCH_DEADLINE_MS: u64 = 5000;
/// Maximum number of workspaces searched concurrently during an `all` fan-out.
pub const FAN_OUT_CONCURRENCY: usize = 4;

/// Whether the server emits the pre-router flat shapes (`Compat`) or the
/// workspace-labeled shapes (`Multi`). Chosen once at startup by activation
/// path, never by the number of registered workspaces (SPEC-0014 R14/R15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    /// Single-workspace compatibility mode (default). Pre-router behavior.
    Compat,
    /// Multi-workspace mode, activated by `--workspaces`.
    Multi,
}

/// How a workspace's effective `Config` was resolved (SPEC-0014 R4/R5),
/// surfaced by the `workspaces` tool so the divergence is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionMode {
    /// Compatibility single-workspace config (the pre-router resolution).
    Compat,
    /// Registry entry with `root` only: workspace config + merged global defaults.
    RootMerge,
    /// Registry entry with `config = …`: pinned file, no global merge.
    PinnedConfig,
}

impl ResolutionMode {
    /// Stable string for operator-facing output.
    pub fn as_str(self) -> &'static str {
        match self {
            ResolutionMode::Compat => "compat",
            ResolutionMode::RootMerge => "root-merge",
            ResolutionMode::PinnedConfig => "pinned-config",
        }
    }
}

/// Two-tier health (SPEC-0014 R60/R61). Cheap validation happens at startup;
/// the SQLite store is opened lazily by the query functions on first use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHealth {
    /// Passed cheap startup validation (id shape, absolute paths, config parse).
    Ok,
    /// Failed validation or initialization; queries are rejected.
    Unavailable(String),
}

impl WorkspaceHealth {
    /// Whether the workspace can currently serve queries.
    pub fn is_ok(&self) -> bool {
        matches!(self, WorkspaceHealth::Ok)
    }
}

/// In-memory runtime for one workspace: its id, root, resolved config, and health.
#[derive(Debug)]
pub struct WorkspaceRuntime {
    /// Stable, user-facing workspace id (`[A-Za-z0-9][A-Za-z0-9_-]*`).
    pub id: String,
    /// Absolute workspace root, if known. `None` for the compat default runtime.
    pub root: Option<PathBuf>,
    /// Whether the workspace accepts queries (a disabled workspace is listed
    /// by discovery but rejects search/get/sources — R7).
    pub enabled: bool,
    /// The resolved effective config; its `[db].path` binds the store (R55–R58).
    pub config: Arc<Config>,
    /// Cheap-validation health state.
    pub health: WorkspaceHealth,
    /// How `config` was resolved (R4/R5), for the `workspaces` tool.
    pub resolution: ResolutionMode,
}

impl WorkspaceRuntime {
    /// The single runtime backing compatibility mode.
    fn compat(config: Arc<Config>) -> Self {
        Self {
            id: "default".to_string(),
            root: None,
            enabled: true,
            config,
            health: WorkspaceHealth::Ok,
            resolution: ResolutionMode::Compat,
        }
    }
}

/// Router error codes (SPEC-0014 R64). Returned to REST as the existing JSON
/// error shape and to MCP as a tool error.
#[derive(Debug, Clone)]
pub enum RouterError {
    /// A request needs an explicit workspace selector (>1 enabled, no default).
    WorkspaceRequired { enabled: Vec<String> },
    /// No workspace exists with the requested id.
    UnknownWorkspace(String),
    /// The requested workspace exists but is disabled.
    WorkspaceDisabled(String),
    /// The requested workspace cannot be loaded or queried.
    WorkspaceUnavailable { id: String, reason: String },
    /// A qualified id conflicts with an explicit `workspace` field.
    WorkspaceIdConflict { field: String, qualified: String },
    /// A selector such as `all` is not valid for the requested operation.
    UnsupportedWorkspaceSelector(String),
    /// A workspace exceeded its per-workspace deadline during an `all` fan-out.
    WorkspaceTimeout { id: String, deadline_ms: u64 },
}

impl RouterError {
    /// The machine-readable error code (SPEC-0014 R64 table).
    pub fn code(&self) -> &'static str {
        match self {
            RouterError::WorkspaceRequired { .. } => "workspace_required",
            RouterError::UnknownWorkspace(_) => "unknown_workspace",
            RouterError::WorkspaceDisabled(_) => "workspace_disabled",
            RouterError::WorkspaceUnavailable { .. } => "workspace_unavailable",
            RouterError::WorkspaceIdConflict { .. } => "workspace_id_conflict",
            RouterError::UnsupportedWorkspaceSelector(_) => "unsupported_workspace_selector",
            RouterError::WorkspaceTimeout { .. } => "workspace_timeout",
        }
    }
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::WorkspaceRequired { enabled } if enabled.is_empty() => write!(
                f,
                "no enabled workspaces are registered; add one with `ctx workspace add`"
            ),
            RouterError::WorkspaceRequired { enabled } => write!(
                f,
                "more than one workspace is enabled and no default is set; pass `workspace` \
                 (one of: {}) or set [defaults].workspace",
                enabled.join(", ")
            ),
            RouterError::UnknownWorkspace(id) => {
                write!(f, "unknown workspace id: {id}")
            }
            RouterError::WorkspaceDisabled(id) => {
                write!(f, "workspace is disabled: {id}")
            }
            RouterError::WorkspaceUnavailable { id, reason } => {
                write!(f, "workspace unavailable: {id} ({reason})")
            }
            RouterError::WorkspaceIdConflict { field, qualified } => write!(
                f,
                "workspace field '{field}' conflicts with qualified id prefix '{qualified}'"
            ),
            RouterError::UnsupportedWorkspaceSelector(sel) => {
                write!(f, "selector '{sel}' is not valid for this operation")
            }
            RouterError::WorkspaceTimeout { id, deadline_ms } => {
                write!(f, "workspace timed out after {deadline_ms}ms: {id}")
            }
        }
    }
}

impl std::error::Error for RouterError {}

/// Routes built-in operations to the selected [`WorkspaceRuntime`].
pub struct WorkspaceRouter {
    default_workspace: Option<String>,
    workspaces: HashMap<String, Arc<WorkspaceRuntime>>,
    /// Stable iteration order for discovery/listing.
    order: Vec<String>,
    mode: ServerMode,
    /// Per-workspace deadline for `all` fan-out (R33). Defaults to
    /// `DEFAULT_SEARCH_DEADLINE_MS`; overridden by `with_search_deadline`.
    search_deadline: Duration,
}

impl WorkspaceRouter {
    /// Build the one-workspace router that backs compatibility mode.
    pub fn single(config: Arc<Config>) -> Self {
        let runtime = Arc::new(WorkspaceRuntime::compat(config));
        let id = runtime.id.clone();
        let mut workspaces = HashMap::new();
        workspaces.insert(id.clone(), runtime);
        Self {
            default_workspace: Some(id.clone()),
            workspaces,
            order: vec![id],
            mode: ServerMode::Compat,
            search_deadline: Duration::from_millis(DEFAULT_SEARCH_DEADLINE_MS),
        }
    }

    /// Build a multi-workspace router from pre-resolved runtimes.
    ///
    /// `runtimes` is consumed in the order it should be listed by discovery.
    /// `default_workspace` is the `[defaults].workspace` id, if any.
    pub fn multi(runtimes: Vec<WorkspaceRuntime>, default_workspace: Option<String>) -> Self {
        let mut workspaces = HashMap::new();
        let mut order = Vec::with_capacity(runtimes.len());
        for rt in runtimes {
            order.push(rt.id.clone());
            workspaces.insert(rt.id.clone(), Arc::new(rt));
        }
        Self {
            default_workspace,
            workspaces,
            order,
            mode: ServerMode::Multi,
            search_deadline: Duration::from_millis(DEFAULT_SEARCH_DEADLINE_MS),
        }
    }

    /// The server mode this router represents.
    pub fn mode(&self) -> ServerMode {
        self.mode
    }

    /// The configured default workspace id, if set.
    pub fn default_id(&self) -> Option<&str> {
        self.default_workspace.as_deref()
    }

    /// Whether `id` is a registered workspace (used for qualified-id parsing, R40).
    pub fn is_registered(&self, id: &str) -> bool {
        self.workspaces.contains_key(id)
    }

    /// All runtimes in stable listing order (for the `workspaces` tool).
    pub fn list(&self) -> Vec<Arc<WorkspaceRuntime>> {
        self.order
            .iter()
            .filter_map(|id| self.workspaces.get(id).cloned())
            .collect()
    }

    /// A config to back convenience-method access in a [`ToolContext`]. Returns
    /// the default workspace's config, else the first enabled, else the first.
    /// In multi mode built-in tools resolve per-call and do not rely on this.
    pub fn default_config(&self) -> Arc<Config> {
        if let Some(rt) = self
            .default_workspace
            .as_ref()
            .and_then(|d| self.workspaces.get(d))
        {
            return rt.config.clone();
        }
        self.order
            .iter()
            .filter_map(|id| self.workspaces.get(id))
            .find(|rt| rt.enabled)
            .or_else(|| self.order.first().and_then(|id| self.workspaces.get(id)))
            .map(|rt| rt.config.clone())
            .expect("WorkspaceRouter must contain at least one workspace")
    }

    /// Ids of all enabled workspaces (used in `workspace_required` messages).
    fn enabled_ids(&self) -> Vec<String> {
        self.order
            .iter()
            .filter(|id| {
                self.workspaces
                    .get(*id)
                    .map(|rt| rt.enabled)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Resolve a selector to a single runtime (SPEC-0014 R19–R26).
    ///
    /// Precedence: explicit id → `[defaults].workspace` → single enabled →
    /// otherwise `workspace_required`. The `all` selector is rejected here in
    /// Phase 1 (`unsupported_workspace_selector`); fan-out is Phase 2.
    pub fn resolve(&self, selector: Option<&str>) -> Result<Arc<WorkspaceRuntime>, RouterError> {
        match selector {
            Some(sel) if sel == ALL_SELECTOR => {
                Err(RouterError::UnsupportedWorkspaceSelector(sel.to_string()))
            }
            Some(id) => self.resolve_id(id),
            None => {
                if let Some(default) = self.default_workspace.clone() {
                    return self.resolve_id(&default);
                }
                let enabled = self.enabled_ids();
                match enabled.as_slice() {
                    [only] => self.resolve_id(only),
                    _ => Err(RouterError::WorkspaceRequired { enabled }),
                }
            }
        }
    }

    /// Resolve a concrete workspace id, applying enabled/health checks.
    fn resolve_id(&self, id: &str) -> Result<Arc<WorkspaceRuntime>, RouterError> {
        let runtime = self
            .workspaces
            .get(id)
            .ok_or_else(|| RouterError::UnknownWorkspace(id.to_string()))?;
        if !runtime.enabled {
            return Err(RouterError::WorkspaceDisabled(id.to_string()));
        }
        match &runtime.health {
            WorkspaceHealth::Ok => Ok(runtime.clone()),
            WorkspaceHealth::Unavailable(reason) => Err(RouterError::WorkspaceUnavailable {
                id: id.to_string(),
                reason: reason.clone(),
            }),
        }
    }

    /// The configured per-workspace `all`-search deadline (R33).
    pub fn search_deadline(&self) -> Duration {
        self.search_deadline
    }

    /// Override the per-workspace `all`-search deadline. `None` leaves the
    /// current value (the default) unchanged. Builder-style so existing
    /// `multi(...)` call sites need no change.
    pub fn with_search_deadline(mut self, ms: Option<u64>) -> Self {
        if let Some(ms) = ms {
            self.search_deadline = Duration::from_millis(ms);
        }
        self
    }

    /// Resolve the `all` selector (SPEC-0014 R21) into the enabled workspaces,
    /// partitioned into healthy runtimes (to search) and enabled-but-unavailable
    /// workspaces as pre-built error entries (R26/R37). Disabled workspaces are
    /// excluded entirely — they are not part of `all` and are not errors (R7).
    /// Output follows the stable registry order.
    pub fn resolve_all(&self) -> (Vec<Arc<WorkspaceRuntime>>, Vec<(String, RouterError)>) {
        let mut healthy = Vec::new();
        let mut errors = Vec::new();
        for id in &self.order {
            let Some(rt) = self.workspaces.get(id) else {
                continue;
            };
            if !rt.enabled {
                continue;
            }
            match &rt.health {
                WorkspaceHealth::Ok => healthy.push(rt.clone()),
                WorkspaceHealth::Unavailable(reason) => errors.push((
                    id.clone(),
                    RouterError::WorkspaceUnavailable {
                        id: id.clone(),
                        reason: reason.clone(),
                    },
                )),
            }
        }
        (healthy, errors)
    }

    /// Split a `get` id into (workspace, raw_id) using qualified-id rules (R40).
    ///
    /// An id is qualified only when it contains `:` and the prefix matches a
    /// **registered** workspace id; otherwise the whole value is a raw id.
    /// Returns the workspace prefix (if qualified) and the bare document id.
    pub fn split_qualified_id<'a>(&self, id: &'a str) -> (Option<&'a str>, &'a str) {
        if let Some((prefix, rest)) = id.split_once(':') {
            if self.is_registered(prefix) {
                return (Some(prefix), rest);
            }
        }
        (None, id)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Workspace registry ($XDG_CONFIG_HOME/ctx/workspaces.toml)
// ═══════════════════════════════════════════════════════════════════════

/// The user-level registry of known workspaces (SPEC-0014 R1/R2).
///
/// Serialized to `$XDG_CONFIG_HOME/ctx/workspaces.toml`. Managed by
/// `ctx workspace add/list/remove`; hand-editing is supported but the CLI
/// validates paths at write time.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WorkspaceRegistry {
    #[serde(default, skip_serializing_if = "RegistryDefaults::is_empty")]
    pub defaults: RegistryDefaults,
    #[serde(default)]
    pub workspaces: BTreeMap<String, WorkspaceEntry>,
}

/// `[defaults]` table of the registry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RegistryDefaults {
    /// Default workspace used when a request omits `workspace` (R19).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Shared server bind address in multi-workspace mode (R16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    /// Per-workspace deadline (ms) for `all` fan-out search (SPEC-0014 R33).
    /// Overrides the built-in 5000 ms default when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_deadline_ms: Option<u64>,
}

impl RegistryDefaults {
    /// Whether no defaults are set (so the `[defaults]` table can be omitted).
    fn is_empty(&self) -> bool {
        self.workspace.is_none() && self.bind.is_none() && self.search_deadline_ms.is_none()
    }
}

/// A single `[workspaces.<id>]` entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceEntry {
    /// Absolute workspace root (R3).
    pub root: PathBuf,
    /// Optional pinned config file; when set it is the sole source, no global
    /// merge (R5). When omitted, the workspace's own `.ctx/config.toml` is
    /// resolved with global defaults merged in (R4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<PathBuf>,
    /// Whether the workspace accepts queries (R6/R7). Defaults to `true`.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Whether `id` is a valid workspace id: `[A-Za-z0-9][A-Za-z0-9_-]*` (R-def).
pub fn is_valid_workspace_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The default registry location: `$XDG_CONFIG_HOME/ctx/workspaces.toml` (R1).
pub fn default_registry_path() -> PathBuf {
    crate::ctx_dirs::config_dir().join("workspaces.toml")
}

impl WorkspaceRegistry {
    /// Parse a registry file. Missing files are an error here; callers decide
    /// whether absence is acceptable (mode is set by `--workspaces`, not file
    /// presence — R10/R12).
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read workspace registry: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("failed to parse workspace registry: {}", path.display()))
    }

    /// Serialize the registry back to TOML (used by `ctx workspace add/remove`).
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string_pretty(self).context("failed to serialize workspace registry")
    }
}

/// Build a multi-workspace [`WorkspaceRouter`] from a parsed registry (R60).
///
/// Cheap validation (id shape, absolute paths, config parse) happens here; an
/// invalid or unresolvable workspace is recorded as
/// [`WorkspaceHealth::Unavailable`] rather than aborting the server (R9/R61).
/// Opening the SQLite store is deferred to the first query.
pub fn build_multi_router(registry: &WorkspaceRegistry) -> anyhow::Result<WorkspaceRouter> {
    if registry.workspaces.is_empty() {
        anyhow::bail!("workspace registry has no [workspaces.*] entries");
    }
    if let Some(default) = &registry.defaults.workspace {
        if !registry.workspaces.contains_key(default) {
            anyhow::bail!("[defaults].workspace = \"{default}\" is not a registered workspace");
        }
    }
    let runtimes = registry
        .workspaces
        .iter()
        .map(|(id, entry)| build_runtime(id, entry))
        .collect();
    Ok(
        WorkspaceRouter::multi(runtimes, registry.defaults.workspace.clone())
            .with_search_deadline(registry.defaults.search_deadline_ms),
    )
}

/// Resolve one registry entry into a runtime, capturing validation/config
/// errors as an unavailable-but-listed runtime.
fn build_runtime(id: &str, entry: &WorkspaceEntry) -> WorkspaceRuntime {
    let resolution = if entry.config.is_some() {
        ResolutionMode::PinnedConfig
    } else {
        ResolutionMode::RootMerge
    };

    let mut problems: Vec<String> = Vec::new();
    if !is_valid_workspace_id(id) {
        problems.push(format!("invalid workspace id '{id}'"));
    }
    if entry.root.is_relative() {
        problems.push(format!("root must be absolute: {}", entry.root.display()));
    }
    if let Some(cfg) = &entry.config {
        if cfg.is_relative() {
            problems.push(format!("config must be absolute: {}", cfg.display()));
        }
    }

    let unavailable = |reason: String| WorkspaceRuntime {
        id: id.to_string(),
        root: Some(entry.root.clone()),
        enabled: entry.enabled,
        config: Arc::new(Config::minimal()),
        health: WorkspaceHealth::Unavailable(reason),
        resolution,
    };

    if !problems.is_empty() {
        return unavailable(problems.join("; "));
    }

    let resolved = match &entry.config {
        Some(cfg) => crate::config::load_config_pinned(cfg, &entry.root),
        None => crate::config::load_config_for_root(&entry.root),
    };

    match resolved {
        Ok(rc) => WorkspaceRuntime {
            id: id.to_string(),
            root: Some(entry.root.clone()),
            enabled: entry.enabled,
            config: Arc::new(rc.config),
            health: WorkspaceHealth::Ok,
            resolution,
        },
        Err(e) => unavailable(format!("config error: {e}")),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// `ctx workspace add/list/remove`
// ═══════════════════════════════════════════════════════════════════════

/// Persist a registry to disk, creating the parent directory if needed.
fn write_registry(path: &Path, registry: &WorkspaceRegistry) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir: {}", parent.display()))?;
    }
    std::fs::write(path, registry.to_toml()?)
        .with_context(|| format!("failed to write workspace registry: {}", path.display()))?;
    Ok(())
}

/// Load the registry from `path`, or a fresh empty one if the file is absent.
fn load_or_default(path: &Path) -> anyhow::Result<WorkspaceRegistry> {
    if path.exists() {
        WorkspaceRegistry::load(path)
    } else {
        Ok(WorkspaceRegistry::default())
    }
}

/// `ctx workspace add` — register a workspace, validating paths at write time.
pub fn cmd_add(
    path: &Path,
    id: &str,
    root: &Path,
    config: Option<&Path>,
    enabled: bool,
) -> anyhow::Result<()> {
    if !is_valid_workspace_id(id) {
        anyhow::bail!("invalid workspace id '{id}': must match [A-Za-z0-9][A-Za-z0-9_-]*");
    }
    // Canonicalize to an absolute, existing path (validate at write time so the
    // registry never holds a relative or dangling root — the design's footgun
    // mitigation).
    let root_abs = std::fs::canonicalize(root)
        .with_context(|| format!("workspace root does not exist: {}", root.display()))?;
    let config_abs = match config {
        Some(c) => Some(
            std::fs::canonicalize(c)
                .with_context(|| format!("config file does not exist: {}", c.display()))?,
        ),
        None => None,
    };

    let mut registry = load_or_default(path)?;
    if registry.workspaces.contains_key(id) {
        anyhow::bail!("workspace '{id}' is already registered; remove it first");
    }
    registry.workspaces.insert(
        id.to_string(),
        WorkspaceEntry {
            root: root_abs,
            config: config_abs,
            enabled,
        },
    );
    write_registry(path, &registry)?;
    println!(
        "Added workspace '{id}'{}",
        if enabled { "" } else { " (disabled)" }
    );
    Ok(())
}

/// `ctx workspace list` — print registered workspaces.
pub fn cmd_list(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        println!("No workspaces registered ({} not found)", path.display());
        return Ok(());
    }
    let registry = WorkspaceRegistry::load(path)?;
    let default = registry.defaults.workspace.as_deref();
    println!("{:<20} {:<8} {:<8} ROOT", "ID", "ENABLED", "DEFAULT");
    for (id, entry) in &registry.workspaces {
        println!(
            "{:<20} {:<8} {:<8} {}",
            id,
            entry.enabled,
            Some(id.as_str()) == default,
            entry.root.display()
        );
    }
    Ok(())
}

/// `ctx workspace remove` — drop a workspace from the registry.
pub fn cmd_remove(path: &Path, id: &str) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("no workspace registry at {}", path.display());
    }
    let mut registry = WorkspaceRegistry::load(path)?;
    if registry.workspaces.remove(id).is_none() {
        anyhow::bail!("workspace '{id}' is not registered");
    }
    // Drop a dangling default pointer.
    if registry.defaults.workspace.as_deref() == Some(id) {
        registry.defaults.workspace = None;
    }
    write_registry(path, &registry)?;
    println!("Removed workspace '{id}'");
    Ok(())
}

/// Outcome of one workspace's fan-out operation.
#[derive(Debug)]
pub(crate) enum FanOut<T> {
    /// The operation completed within the deadline.
    Ok(T),
    /// The operation returned an error (message captured for the `errors[]` entry).
    Failed(String),
    /// The operation exceeded the per-item deadline.
    TimedOut,
}

/// Run `op` for each `(id, input)` under a concurrency cap and a per-item
/// deadline, returning outcomes paired with their id in the input order.
///
/// No input id is ever dropped: a task that panics is reported as `Failed`.
/// This backs SPEC-0014 R33 (per-workspace deadline) and R37 (failures surfaced,
/// never silently removed). It is generic over the op so the timeout/concurrency
/// behavior is unit-tested with injected futures — no real store required.
pub(crate) async fn fan_out<I, T, F, Fut>(
    items: Vec<(String, I)>,
    deadline: Duration,
    max_concurrency: usize,
    op: F,
) -> Vec<(String, FanOut<T>)>
where
    I: Send + 'static,
    T: Send + 'static,
    F: Fn(I) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
{
    let order: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
    let sem = Arc::new(Semaphore::new(max_concurrency.max(1)));
    let mut set: JoinSet<(String, FanOut<T>)> = JoinSet::new();
    for (id, input) in items {
        let sem = sem.clone();
        let op = op.clone();
        set.spawn(async move {
            let _permit = sem
                .acquire_owned()
                .await
                .expect("fan-out semaphore is never closed");
            let outcome = match tokio::time::timeout(deadline, op(input)).await {
                Ok(Ok(value)) => FanOut::Ok(value),
                Ok(Err(e)) => FanOut::Failed(e.to_string()),
                Err(_) => FanOut::TimedOut,
            };
            (id, outcome)
        });
    }

    let mut by_id: HashMap<String, FanOut<T>> = HashMap::new();
    while let Some(joined) = set.join_next().await {
        if let Ok((id, outcome)) = joined {
            by_id.insert(id, outcome);
        }
        // A JoinError (panic/cancel) leaves the id missing; it is reconciled
        // below as `Failed` so the workspace is never dropped (R37).
    }

    order
        .into_iter()
        .map(|id| {
            let outcome = by_id
                .remove(&id)
                .unwrap_or_else(|| FanOut::Failed("workspace task did not complete".to_string()));
            (id, outcome)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Arc<Config> {
        // A minimal config is enough; resolution-mode/health are what we test.
        Arc::new(crate::config::Config::minimal())
    }

    fn runtime(id: &str, enabled: bool, health: WorkspaceHealth) -> WorkspaceRuntime {
        WorkspaceRuntime {
            id: id.to_string(),
            root: Some(PathBuf::from(format!("/abs/{id}"))),
            enabled,
            config: cfg(),
            health,
            resolution: ResolutionMode::RootMerge,
        }
    }

    #[test]
    fn single_router_resolves_default() {
        let r = WorkspaceRouter::single(cfg());
        assert_eq!(r.mode(), ServerMode::Compat);
        let rt = r.resolve(None).unwrap();
        assert_eq!(rt.id, "default");
    }

    #[test]
    fn multi_requires_selector_when_ambiguous() {
        let r = WorkspaceRouter::multi(
            vec![
                runtime("a", true, WorkspaceHealth::Ok),
                runtime("b", true, WorkspaceHealth::Ok),
            ],
            None,
        );
        let err = r.resolve(None).unwrap_err();
        assert_eq!(err.code(), "workspace_required");
    }

    #[test]
    fn multi_single_enabled_used_without_default() {
        let r = WorkspaceRouter::multi(
            vec![
                runtime("a", true, WorkspaceHealth::Ok),
                runtime("b", false, WorkspaceHealth::Ok),
            ],
            None,
        );
        assert_eq!(r.resolve(None).unwrap().id, "a");
    }

    #[test]
    fn multi_default_is_used() {
        let r = WorkspaceRouter::multi(
            vec![
                runtime("a", true, WorkspaceHealth::Ok),
                runtime("b", true, WorkspaceHealth::Ok),
            ],
            Some("b".to_string()),
        );
        assert_eq!(r.resolve(None).unwrap().id, "b");
    }

    #[test]
    fn unknown_disabled_unavailable_errors() {
        let r = WorkspaceRouter::multi(
            vec![
                runtime("a", true, WorkspaceHealth::Ok),
                runtime("b", false, WorkspaceHealth::Ok),
                runtime("c", true, WorkspaceHealth::Unavailable("bad config".into())),
            ],
            None,
        );
        assert_eq!(
            r.resolve(Some("nope")).unwrap_err().code(),
            "unknown_workspace"
        );
        assert_eq!(
            r.resolve(Some("b")).unwrap_err().code(),
            "workspace_disabled"
        );
        assert_eq!(
            r.resolve(Some("c")).unwrap_err().code(),
            "workspace_unavailable"
        );
    }

    #[test]
    fn all_selector_rejected_in_phase1() {
        let r = WorkspaceRouter::multi(vec![runtime("a", true, WorkspaceHealth::Ok)], None);
        assert_eq!(
            r.resolve(Some("all")).unwrap_err().code(),
            "unsupported_workspace_selector"
        );
    }

    #[test]
    fn qualified_id_only_for_registered_prefix() {
        let r = WorkspaceRouter::multi(
            vec![runtime("context_harness", true, WorkspaceHealth::Ok)],
            None,
        );
        assert_eq!(
            r.split_qualified_id("context_harness:01J"),
            (Some("context_harness"), "01J")
        );
        // Unregistered prefix -> treated as a raw id.
        assert_eq!(r.split_qualified_id("foo:bar"), (None, "foo:bar"));
        assert_eq!(r.split_qualified_id("01Jabc"), (None, "01Jabc"));
    }

    #[test]
    fn workspace_id_validation() {
        assert!(is_valid_workspace_id("context_harness"));
        assert!(is_valid_workspace_id("stack-app"));
        assert!(is_valid_workspace_id("a"));
        assert!(is_valid_workspace_id("ws1"));
        assert!(!is_valid_workspace_id(""));
        assert!(!is_valid_workspace_id("_leading"));
        assert!(!is_valid_workspace_id("-leading"));
        assert!(!is_valid_workspace_id("has space"));
        assert!(!is_valid_workspace_id("has:colon"));
        assert!(!is_valid_workspace_id("has/slash"));
    }

    /// Write a minimal workspace at `<root>/.ctx/config.toml` with a
    /// workspace-relative db path, and return the root.
    fn seed_workspace(root: &Path) {
        let ctx = root.join(".ctx");
        std::fs::create_dir_all(ctx.join("data")).unwrap();
        std::fs::write(
            ctx.join("config.toml"),
            "[db]\npath = \".ctx/data/ctx.sqlite\"\n\n\
             [chunking]\nmax_tokens = 700\noverlap_tokens = 0\n\n\
             [retrieval]\nfinal_limit = 12\n\n\
             [server]\nbind = \"127.0.0.1:7331\"\n",
        )
        .unwrap();
    }

    #[test]
    fn two_workspaces_resolve_to_distinct_absolute_db_paths() {
        // SPEC-0014 R55–R58 + the plan's re-rooting risk check: each workspace
        // must address its own store, anchored at its own root (not the cwd).
        let tmp_a = tempfile::TempDir::new().unwrap();
        let tmp_b = tempfile::TempDir::new().unwrap();
        seed_workspace(tmp_a.path());
        seed_workspace(tmp_b.path());

        let mut workspaces = BTreeMap::new();
        workspaces.insert(
            "a".to_string(),
            WorkspaceEntry {
                root: tmp_a.path().to_path_buf(),
                config: None,
                enabled: true,
            },
        );
        workspaces.insert(
            "b".to_string(),
            WorkspaceEntry {
                root: tmp_b.path().to_path_buf(),
                config: None,
                enabled: true,
            },
        );
        let registry = WorkspaceRegistry {
            defaults: RegistryDefaults::default(),
            workspaces,
        };

        let router = build_multi_router(&registry).unwrap();
        let a = router.resolve(Some("a")).unwrap();
        let b = router.resolve(Some("b")).unwrap();

        assert!(a.config.db.path.is_absolute(), "a db path must be absolute");
        assert!(b.config.db.path.is_absolute(), "b db path must be absolute");
        assert_ne!(
            a.config.db.path, b.config.db.path,
            "stores must be distinct"
        );
        assert!(a.config.db.path.starts_with(tmp_a.path()));
        assert!(b.config.db.path.starts_with(tmp_b.path()));
        assert_eq!(a.resolution, ResolutionMode::RootMerge);
    }

    #[test]
    fn relative_root_makes_workspace_unavailable() {
        let mut workspaces = BTreeMap::new();
        workspaces.insert(
            "rel".to_string(),
            WorkspaceEntry {
                root: PathBuf::from("relative/path"),
                config: None,
                enabled: true,
            },
        );
        let registry = WorkspaceRegistry {
            defaults: RegistryDefaults::default(),
            workspaces,
        };
        let router = build_multi_router(&registry).unwrap();
        // Listed by discovery, but queries are rejected as unavailable (R9).
        assert_eq!(router.list().len(), 1);
        assert_eq!(
            router.resolve(Some("rel")).unwrap_err().code(),
            "workspace_unavailable"
        );
    }

    #[test]
    fn cmd_add_list_remove_roundtrip() {
        let cfg_tmp = tempfile::TempDir::new().unwrap();
        let reg_path = cfg_tmp.path().join("workspaces.toml");
        let ws_a = tempfile::TempDir::new().unwrap();
        let ws_b = tempfile::TempDir::new().unwrap();

        cmd_add(&reg_path, "alpha", ws_a.path(), None, true).unwrap();
        cmd_add(&reg_path, "beta", ws_b.path(), None, false).unwrap();

        let reg = WorkspaceRegistry::load(&reg_path).unwrap();
        assert_eq!(reg.workspaces.len(), 2);
        assert!(reg.workspaces["alpha"].enabled);
        assert!(!reg.workspaces["beta"].enabled);
        assert!(reg.workspaces["alpha"].root.is_absolute());

        // Duplicate id, invalid id, and nonexistent root are all rejected.
        assert!(cmd_add(&reg_path, "alpha", ws_a.path(), None, true).is_err());
        assert!(cmd_add(&reg_path, "bad id", ws_a.path(), None, true).is_err());
        assert!(cmd_add(
            &reg_path,
            "gamma",
            Path::new("/no/such/path/xyz123"),
            None,
            true
        )
        .is_err());

        cmd_remove(&reg_path, "beta").unwrap();
        let reg = WorkspaceRegistry::load(&reg_path).unwrap();
        assert_eq!(reg.workspaces.len(), 1);
        assert!(cmd_remove(&reg_path, "beta").is_err());
    }

    #[test]
    fn registry_roundtrips_through_toml() {
        let toml_src = "[defaults]\nworkspace = \"a\"\n\n\
                        [workspaces.a]\nroot = \"/abs/a\"\nenabled = true\n";
        let reg: WorkspaceRegistry = toml::from_str(toml_src).unwrap();
        assert_eq!(reg.defaults.workspace.as_deref(), Some("a"));
        assert_eq!(reg.workspaces["a"].root, PathBuf::from("/abs/a"));
        assert!(reg.workspaces["a"].enabled);
        // Round-trip back to TOML and re-parse.
        let out = reg.to_toml().unwrap();
        let reparsed: WorkspaceRegistry = toml::from_str(&out).unwrap();
        assert_eq!(reparsed.workspaces["a"].root, PathBuf::from("/abs/a"));
    }

    #[test]
    fn registry_parses_search_deadline_ms() {
        let toml = "[defaults]\nworkspace = \"a\"\nsearch_deadline_ms = 1234\n\n\
                    [workspaces.a]\nroot = \"/abs/a\"\nenabled = true\n";
        let reg: WorkspaceRegistry = toml::from_str(toml).unwrap();
        assert_eq!(reg.defaults.search_deadline_ms, Some(1234));
        // Round-trips and is omitted when unset.
        let reg2 = WorkspaceRegistry {
            defaults: RegistryDefaults::default(),
            workspaces: reg.workspaces.clone(),
        };
        let out = toml::to_string(&reg2).unwrap();
        assert!(
            !out.contains("search_deadline_ms"),
            "unset field is omitted"
        );
    }

    #[test]
    fn workspace_timeout_error_code() {
        let e = RouterError::WorkspaceTimeout {
            id: "beta".to_string(),
            deadline_ms: 5000,
        };
        assert_eq!(e.code(), "workspace_timeout");
        assert!(e.to_string().contains("beta"));
        assert!(e.to_string().contains("5000"));
    }

    #[test]
    fn resolve_all_partitions_by_enabled_and_health() {
        let r = WorkspaceRouter::multi(
            vec![
                runtime("a", true, WorkspaceHealth::Ok),
                runtime("b", false, WorkspaceHealth::Ok), // disabled -> excluded
                runtime("c", true, WorkspaceHealth::Unavailable("boom".to_string())),
                runtime("d", true, WorkspaceHealth::Ok),
            ],
            None,
        );
        let (healthy, errors) = r.resolve_all();
        // Healthy, in registry order: a, d. Disabled b excluded.
        assert_eq!(
            healthy.iter().map(|rt| rt.id.clone()).collect::<Vec<_>>(),
            vec!["a".to_string(), "d".to_string()]
        );
        // c is enabled-but-unavailable -> one error entry.
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "c");
        assert_eq!(errors[0].1.code(), "workspace_unavailable");
    }

    #[test]
    fn with_search_deadline_overrides_default() {
        let r = WorkspaceRouter::multi(vec![runtime("a", true, WorkspaceHealth::Ok)], None);
        assert_eq!(
            r.search_deadline().as_millis(),
            DEFAULT_SEARCH_DEADLINE_MS as u128
        );
        let r = r.with_search_deadline(Some(250));
        assert_eq!(r.search_deadline().as_millis(), 250);
        // None leaves it unchanged.
        let r = r.with_search_deadline(None);
        assert_eq!(r.search_deadline().as_millis(), 250);
    }

    #[tokio::test]
    async fn fan_out_classifies_and_preserves_order() {
        let items = vec![
            ("ok".to_string(), 1u32),
            ("fail".to_string(), 2u32),
            ("slow".to_string(), 3u32),
        ];
        let outcomes = fan_out(items, Duration::from_millis(50), 4, |n: u32| async move {
            match n {
                1 => Ok("done".to_string()),
                2 => Err(anyhow::anyhow!("boom")),
                _ => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    Ok("late".to_string())
                }
            }
        })
        .await;

        assert_eq!(
            outcomes
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            vec!["ok".to_string(), "fail".to_string(), "slow".to_string()],
            "order follows input order"
        );
        assert!(matches!(&outcomes[0].1, FanOut::Ok(s) if s == "done"));
        assert!(matches!(&outcomes[1].1, FanOut::Failed(m) if m.contains("boom")));
        assert!(matches!(&outcomes[2].1, FanOut::TimedOut));
    }
}
