# Multi-Workspace Router Phase 2 — All-Workspace Fan-Out Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `workspace = "all"` fan-out for the built-in `search` and `sources` tools in multi-workspace mode: query every enabled workspace concurrently under a per-workspace deadline, return grouped results, and report each failed/timed-out workspace in an `errors[]` array.

**Architecture:** A generic, unit-testable `fan_out` helper (bounded concurrency via `tokio::Semaphore` + per-item `tokio::time::timeout`) lives in the router module. `WorkspaceRouter::resolve_all()` partitions enabled workspaces into healthy runtimes (searched) and enabled-but-unavailable ones (seeded as errors); disabled workspaces are excluded. The multi-mode `RoutedSearchTool`/`RoutedSourcesTool` branch on the `all` selector and assemble the existing grouped wire shape (`{results:[{workspace,items}], errors:[]}`) with N groups. `resolve()` continues to reject `all` for the single-target path, so compatibility mode and single-workspace routing are untouched.

**Tech Stack:** Rust, `tokio` (already `features = ["full"]` — provides `Semaphore`, `time::timeout`, `JoinSet`, `#[tokio::test]`), `serde_json`, `anyhow`, `async-trait`.

## Global Constraints

- **Additive invariant (SPEC-0014 R15):** do NOT modify compatibility-mode code paths or the flat single-workspace shape. The non-routed `SearchTool`/`GetTool`/`SourcesTool` and the single-workspace branches of the `Routed*` tools stay byte-for-byte unchanged. Only the `all`-selector branches are new.
- **No new crate dependencies.** Use `tokio` only. Do NOT add `futures`/`futures-util`.
- **`limit` applies per workspace (R34); never compare scores across stores (R35).** Each workspace search receives the same `limit`; no global merge or re-rank.
- **Failures are never silently dropped (R37).** Every enabled workspace that fails, is unavailable, or exceeds its deadline contributes exactly one entry to `errors[]`. Disabled workspaces are excluded from `all` and are NOT errors (R7/R21).
- **Deterministic output.** Groups and error entries follow the router's stable registry order (`WorkspaceRouter.order`).
- **Error code string is exactly `workspace_timeout`.**
- **Error-entry shape is exactly** `{ "workspace": "<id>", "code": "<code>", "message": "<text>" }`.
- **Redaction (R48/R49) is preserved** automatically by reusing `get_sources` for `sources = "all"`.
- **Deadline default 5000 ms**, overridable via `[defaults].search_deadline_ms` in `workspaces.toml`. **Concurrency cap = 4.**
- Successful REST tool responses are wrapped under `["result"]`; router errors under `["error"]["code"]`.

---

## Task 1: Registry config knob — `[defaults].search_deadline_ms`

**Files:**
- Modify: `crates/context-harness/src/workspace.rs` (`RegistryDefaults` struct + `is_empty`, ~lines 355-371)
- Test: `crates/context-harness/src/workspace.rs` (`#[cfg(test)] mod tests`, add a test)

**Interfaces:**
- Produces: `RegistryDefaults.search_deadline_ms: Option<u64>` (consumed by Task 3).

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `crates/context-harness/src/workspace.rs`:

```rust
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
        assert!(!out.contains("search_deadline_ms"), "unset field is omitted");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p context-harness registry_parses_search_deadline_ms`
Expected: FAIL — compile error `no field search_deadline_ms on RegistryDefaults`.

- [ ] **Step 3: Add the field**

In `RegistryDefaults` (after the `bind` field):

```rust
    /// Per-workspace deadline (ms) for `all` fan-out search (SPEC-0014 R33).
    /// Overrides the built-in 5000 ms default when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_deadline_ms: Option<u64>,
```

Update `is_empty` to account for it:

```rust
    fn is_empty(&self) -> bool {
        self.workspace.is_none() && self.bind.is_none() && self.search_deadline_ms.is_none()
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p context-harness registry_parses_search_deadline_ms`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/context-harness/src/workspace.rs
git commit -m "feat(workspace): add [defaults].search_deadline_ms registry knob"
```

---

## Task 2: New error code `workspace_timeout`

**Files:**
- Modify: `crates/context-harness/src/workspace.rs` (`RouterError` enum ~line 117, `code()` ~line 133, `Display` ~line 148)
- Modify: `crates/context-harness/src/server.rs` (`classify_tool_error` match ~line 507)
- Test: `crates/context-harness/src/workspace.rs` (`mod tests`)

**Interfaces:**
- Produces: `RouterError::WorkspaceTimeout { id: String, deadline_ms: u64 }` with `code() == "workspace_timeout"` (consumed by Task 5).

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `workspace.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p context-harness workspace_timeout_error_code`
Expected: FAIL — `no variant named WorkspaceTimeout`.

- [ ] **Step 3: Add the variant + code + Display**

Add the variant to the `RouterError` enum (after `UnsupportedWorkspaceSelector`):

```rust
    /// A workspace exceeded its per-workspace deadline during an `all` fan-out.
    WorkspaceTimeout { id: String, deadline_ms: u64 },
```

Add the `code()` arm (the match has no `_`, so this arm is required):

```rust
            RouterError::WorkspaceTimeout { .. } => "workspace_timeout",
```

Add the `Display` arm (the match has no `_`, so this arm is required):

```rust
            RouterError::WorkspaceTimeout { id, deadline_ms } => {
                write!(f, "workspace timed out after {deadline_ms}ms: {id}")
            }
```

- [ ] **Step 4: Add the HTTP status mapping (defensive)**

In `crates/context-harness/src/server.rs`, `classify_tool_error`, extend the status match so a timeout maps to 503 if it ever surfaces top-level (it normally appears only inside `errors[]`):

```rust
        let status = match re {
            RouterError::UnknownWorkspace(_) => StatusCode::NOT_FOUND,
            RouterError::WorkspaceUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            RouterError::WorkspaceTimeout { .. } => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::BAD_REQUEST,
        };
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p context-harness workspace_timeout_error_code`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/context-harness/src/workspace.rs crates/context-harness/src/server.rs
git commit -m "feat(workspace): add workspace_timeout RouterError code"
```

---

## Task 3: Router deadline + `resolve_all()`

**Files:**
- Modify: `crates/context-harness/src/workspace.rs` (top-of-file `use`, consts near `ALL_SELECTOR` ~line 28, `WorkspaceRouter` struct ~line 182, `single()` ~line 192, `multi()` ~line 209, add methods after `resolve_id`, `build_multi_router` ~line 430)
- Test: `crates/context-harness/src/workspace.rs` (`mod tests`)

**Interfaces:**
- Consumes: `RegistryDefaults.search_deadline_ms` (Task 1).
- Produces:
  - `pub const DEFAULT_SEARCH_DEADLINE_MS: u64 = 5000;`
  - `pub const FAN_OUT_CONCURRENCY: usize = 4;`
  - `WorkspaceRouter::resolve_all(&self) -> (Vec<Arc<WorkspaceRuntime>>, Vec<(String, RouterError)>)`
  - `WorkspaceRouter::search_deadline(&self) -> std::time::Duration`
  - `WorkspaceRouter::with_search_deadline(self, ms: Option<u64>) -> Self`
  (all consumed by Task 5.)

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `workspace.rs`:

```rust
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
        assert_eq!(r.search_deadline().as_millis(), DEFAULT_SEARCH_DEADLINE_MS as u128);
        let r = r.with_search_deadline(Some(250));
        assert_eq!(r.search_deadline().as_millis(), 250);
        // None leaves it unchanged.
        let r = r.with_search_deadline(None);
        assert_eq!(r.search_deadline().as_millis(), 250);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p context-harness resolve_all_partitions_by_enabled_and_health with_search_deadline_overrides_default`
Expected: FAIL — `no method resolve_all` / `no method search_deadline`.

- [ ] **Step 3: Add the import + constants**

At the top of `workspace.rs`, add (near the other `std` imports):

```rust
use std::time::Duration;
```

Below the existing `ALL_SELECTOR` const, add:

```rust
/// Default per-workspace deadline for an `all` fan-out search (SPEC-0014 R33).
pub const DEFAULT_SEARCH_DEADLINE_MS: u64 = 5000;
/// Maximum number of workspaces searched concurrently during an `all` fan-out.
pub const FAN_OUT_CONCURRENCY: usize = 4;
```

- [ ] **Step 4: Add the router field and set it in both constructors**

Add a field to the `WorkspaceRouter` struct (after `mode`):

```rust
    /// Per-workspace deadline for `all` fan-out (R33). Defaults to
    /// `DEFAULT_SEARCH_DEADLINE_MS`; overridden by `with_search_deadline`.
    search_deadline: Duration,
```

In `single(...)`, add to the returned struct literal (after `mode: ServerMode::Compat,`):

```rust
            search_deadline: Duration::from_millis(DEFAULT_SEARCH_DEADLINE_MS),
```

In `multi(...)`, add to the returned struct literal (after `mode: ServerMode::Multi,`):

```rust
            search_deadline: Duration::from_millis(DEFAULT_SEARCH_DEADLINE_MS),
```

- [ ] **Step 5: Add the methods**

Immediately after `resolve_id(...)` (before `split_qualified_id`), add:

```rust
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
```

- [ ] **Step 6: Wire the registry knob into the router**

In `build_multi_router`, change the returned `Ok(...)` to apply the deadline override:

```rust
    Ok(
        WorkspaceRouter::multi(runtimes, registry.defaults.workspace.clone())
            .with_search_deadline(registry.defaults.search_deadline_ms),
    )
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p context-harness resolve_all_partitions_by_enabled_and_health with_search_deadline_overrides_default`
Expected: PASS. Also run `cargo test -p context-harness --lib workspace` to confirm no existing router test regressed (e.g. `all_selector_rejected_in_phase1` still passes — `resolve()` is unchanged).

- [ ] **Step 8: Commit**

```bash
git add crates/context-harness/src/workspace.rs
git commit -m "feat(workspace): add resolve_all and configurable fan-out deadline"
```

---

## Task 4: Generic `fan_out` helper

**Files:**
- Modify: `crates/context-harness/src/workspace.rs` (top-of-file `use`; add `FanOut` enum + `fan_out` fn — place them just above the `#[cfg(test)]` module)
- Test: `crates/context-harness/src/workspace.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `pub(crate) enum FanOut<T> { Ok(T), Failed(String), TimedOut }`
  - `pub(crate) async fn fan_out<I, T, F, Fut>(items: Vec<(String, I)>, deadline: Duration, max_concurrency: usize, op: F) -> Vec<(String, FanOut<T>)>` where `I: Send + 'static`, `T: Send + 'static`, `F: Fn(I) -> Fut + Clone + Send + 'static`, `Fut: Future<Output = anyhow::Result<T>> + Send + 'static`
  (consumed by Task 5). Results are returned in the same order as `items`; a timed-out item yields `TimedOut`, an `Err` yields `Failed(msg)`, and no input id is ever dropped.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `workspace.rs`:

```rust
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
            outcomes.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            vec!["ok".to_string(), "fail".to_string(), "slow".to_string()],
            "order follows input order"
        );
        assert!(matches!(&outcomes[0].1, FanOut::Ok(s) if s == "done"));
        assert!(matches!(&outcomes[1].1, FanOut::Failed(m) if m.contains("boom")));
        assert!(matches!(&outcomes[2].1, FanOut::TimedOut));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p context-harness fan_out_classifies_and_preserves_order`
Expected: FAIL — `cannot find function fan_out` / `cannot find type FanOut`.

- [ ] **Step 3: Add the imports**

At the top of `workspace.rs`, add:

```rust
use std::future::Future;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
```

(`Duration` and `HashMap` are already imported; `Arc` is already imported.)

- [ ] **Step 4: Add the enum and helper**

Place immediately above `#[cfg(test)]`:

```rust
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p context-harness fan_out_classifies_and_preserves_order`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/context-harness/src/workspace.rs
git commit -m "feat(workspace): add bounded fan_out helper with per-item deadline"
```

---

## Task 5: Wire `all` fan-out into the built-in tools

**Files:**
- Modify: `crates/context-harness/src/traits.rs` (the `use crate::workspace::{...}` line ~57; refactor `shape_grouped_search` ~517; add `search_group`, `error_entry`, `search_all`; extend `RoutedSearchTool::execute` ~567 and `RoutedSourcesTool::execute` ~679)
- Test: `crates/context-harness/src/traits.rs` (add a `#[cfg(test)] mod tests` if none exists, or extend it — see Step 1)

**Interfaces:**
- Consumes: `RouterError::WorkspaceTimeout` (Task 2); `WorkspaceRouter::resolve_all`, `search_deadline`, `FAN_OUT_CONCURRENCY` (Task 3); `fan_out`, `FanOut` (Task 4); `ALL_SELECTOR`, `search_documents`, `get_sources`, `SearchResultItem` (existing, already imported).
- Produces: `all`-selector behavior for the `search` and `sources` built-ins; the pure helpers `search_group` and `error_entry`.

- [ ] **Step 1: Write the failing test**

At the bottom of `crates/context-harness/src/traits.rs`, add (or extend an existing `mod tests`):

```rust
#[cfg(test)]
mod phase2_tests {
    use super::*;
    use crate::workspace::RouterError;

    #[test]
    fn search_group_tags_items_with_workspace_and_qualified_id() {
        let item = SearchResultItem {
            id: "01ABC".to_string(),
            score: 0.5,
            title: Some("T".to_string()),
            source: "filesystem".to_string(),
            source_id: "f".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            snippet: "snip".to_string(),
            source_url: None,
            explain: None,
        };
        let group = search_group("beta", vec![item]);
        assert_eq!(group["workspace"], "beta");
        let items = group["items"].as_array().unwrap();
        assert_eq!(items[0]["workspace"], "beta");
        assert_eq!(items[0]["qualified_id"], "beta:01ABC");
    }

    #[test]
    fn error_entry_has_workspace_code_message() {
        let err = RouterError::WorkspaceTimeout {
            id: "beta".to_string(),
            deadline_ms: 5000,
        };
        let entry = error_entry("beta", &err);
        assert_eq!(entry["workspace"], "beta");
        assert_eq!(entry["code"], "workspace_timeout");
        assert!(entry["message"].as_str().unwrap().contains("5000"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p context-harness search_group_tags_items_with_workspace_and_qualified_id error_entry_has_workspace_code_message`
Expected: FAIL — `cannot find function search_group` / `error_entry`.

- [ ] **Step 3: Extend the imports**

Change the workspace import line in `traits.rs` from:

```rust
use crate::workspace::{RouterError, ServerMode, WorkspaceRouter};
```

to:

```rust
use crate::workspace::{
    fan_out, FanOut, RouterError, ServerMode, WorkspaceRouter, ALL_SELECTOR, FAN_OUT_CONCURRENCY,
};
```

- [ ] **Step 4: Refactor `shape_grouped_search` into a reusable group builder**

Replace the existing `shape_grouped_search` function body with a thin wrapper over a new `search_group`, and add `error_entry`:

```rust
/// Build one `{ workspace, items }` group, tagging every item with `workspace`
/// and a `qualified_id` (R29/R30/R36).
fn search_group(workspace: &str, results: Vec<SearchResultItem>) -> Value {
    let items: Vec<Value> = results
        .into_iter()
        .map(|item| {
            let mut obj = serde_json::to_value(&item).unwrap_or(Value::Null);
            if let Value::Object(map) = &mut obj {
                let doc_id = map
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                map.insert("workspace".to_string(), Value::String(workspace.to_string()));
                map.insert(
                    "qualified_id".to_string(),
                    Value::String(format!("{workspace}:{doc_id}")),
                );
            }
            obj
        })
        .collect();
    serde_json::json!({ "workspace": workspace, "items": items })
}

/// A single-workspace grouped response: one group, no errors (R29/R30).
fn shape_grouped_search(workspace: &str, results: Vec<SearchResultItem>) -> Value {
    serde_json::json!({ "results": [ search_group(workspace, results) ], "errors": [] })
}

/// Build one `errors[]` entry for a failed workspace (R37):
/// `{ workspace, code, message }`.
fn error_entry(workspace: &str, err: &RouterError) -> Value {
    serde_json::json!({
        "workspace": workspace,
        "code": err.code(),
        "message": err.to_string(),
    })
}
```

- [ ] **Step 5: Add the `search_all` fan-out function**

Add near `RoutedSearchTool` (below `shape_grouped_search`):

```rust
/// Fan out a `search` across every enabled workspace (R32–R37): concurrent,
/// per-workspace deadline, per-workspace `limit`, grouped results, and one
/// `errors[]` entry per failed/timed-out workspace.
async fn search_all(
    ctx: &ToolContext,
    query: String,
    mode: String,
    source: Option<String>,
    since: Option<String>,
    limit: i64,
) -> Result<Value> {
    let router = ctx.router();
    let (healthy, seed_errors) = router.resolve_all();
    let deadline = router.search_deadline();
    let deadline_ms = deadline.as_millis() as u64;

    let items: Vec<(String, Arc<Config>)> = healthy
        .iter()
        .map(|rt| (rt.id.clone(), rt.config.clone()))
        .collect();

    let outcomes = fan_out(items, deadline, FAN_OUT_CONCURRENCY, move |config: Arc<Config>| {
        let query = query.clone();
        let mode = mode.clone();
        let source = source.clone();
        let since = since.clone();
        async move {
            search_documents(
                &config,
                &query,
                &mode,
                source.as_deref(),
                since.as_deref(),
                Some(limit),
                false,
            )
            .await
        }
    })
    .await;

    let mut groups: Vec<Value> = Vec::new();
    let mut errors: Vec<Value> = seed_errors
        .iter()
        .map(|(id, err)| error_entry(id, err))
        .collect();

    for (id, outcome) in outcomes {
        match outcome {
            FanOut::Ok(results) => groups.push(search_group(&id, results)),
            FanOut::Failed(msg) => errors.push(error_entry(
                &id,
                &RouterError::WorkspaceUnavailable {
                    id: id.clone(),
                    reason: msg,
                },
            )),
            FanOut::TimedOut => errors.push(error_entry(
                &id,
                &RouterError::WorkspaceTimeout {
                    id: id.clone(),
                    deadline_ms,
                },
            )),
        }
    }

    Ok(serde_json::json!({ "results": groups, "errors": errors }))
}
```

- [ ] **Step 6: Branch `RoutedSearchTool::execute` on `all`**

In `RoutedSearchTool::execute`, after the `selector` is read and before `let runtime = ctx.router().resolve(selector)?;`, insert the `all` branch:

```rust
        let selector = params["workspace"].as_str();
        if selector == Some(ALL_SELECTOR) {
            return search_all(
                ctx,
                query.to_string(),
                mode.to_string(),
                source.map(str::to_string),
                since.map(str::to_string),
                limit,
            )
            .await;
        }
        let runtime = ctx.router().resolve(selector)?;
```

(The existing single-workspace lines below — `search_documents(...)` and `shape_grouped_search(...)` — are unchanged.)

- [ ] **Step 7: Branch `RoutedSourcesTool::execute` on `all`**

In `RoutedSourcesTool::execute`, after `let selector = params["workspace"].as_str();`, insert:

```rust
        let selector = params["workspace"].as_str();
        if selector == Some(ALL_SELECTOR) {
            let (healthy, seed_errors) = ctx.router().resolve_all();
            let results: Vec<Value> = healthy
                .iter()
                .map(|rt| serde_json::json!({ "workspace": rt.id, "sources": get_sources(&rt.config) }))
                .collect();
            let errors: Vec<Value> = seed_errors
                .iter()
                .map(|(id, err)| error_entry(id, err))
                .collect();
            return Ok(serde_json::json!({ "results": results, "errors": errors }));
        }
        let runtime = ctx.router().resolve(selector)?;
```

(The existing single-workspace lines below are unchanged.)

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p context-harness search_group_tags_items_with_workspace_and_qualified_id error_entry_has_workspace_code_message`
Expected: PASS.
Then `cargo build -p context-harness` to confirm the whole crate compiles.
Expected: builds clean (no warnings about unused `fan_out`/`FanOut`/`ALL_SELECTOR`).

- [ ] **Step 9: Commit**

```bash
git add crates/context-harness/src/traits.rs
git commit -m "feat(router): implement workspace=all fan-out for search and sources"
```

---

## Task 6: End-to-end integration test

**Files:**
- Modify: `crates/context-harness/tests/integration.rs` (`test_multi_workspace_routing`, replace the `all`-rejection block ~1585-1593)

**Interfaces:**
- Consumes: the running multi-workspace server behavior from Task 5.

- [ ] **Step 1: Replace the Phase-1 `all`-rejection assertion with fan-out assertions**

In `test_multi_workspace_routing`, replace the block:

```rust
    // `all` is rejected in Phase 1.
    let all: serde_json::Value = client
        .post(format!("{}/tools/search", base))
        .json(&serde_json::json!({ "query": "x", "workspace": "all" }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(all["error"]["code"], "unsupported_workspace_selector");
```

with:

```rust
    // `all` fans out across enabled workspaces (Phase 2). Both alpha and beta
    // docs contain "notes"; gamma is disabled and excluded.
    let all: serde_json::Value = client
        .post(format!("{}/tools/search", base))
        .json(&serde_json::json!({ "query": "notes", "workspace": "all", "limit": 5 }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let groups = all["result"]["results"].as_array().unwrap();
    let ws_ids: Vec<&str> = groups.iter().map(|g| g["workspace"].as_str().unwrap()).collect();
    assert!(ws_ids.contains(&"alpha"), "alpha present in all-search: {ws_ids:?}");
    assert!(ws_ids.contains(&"beta"), "beta present in all-search: {ws_ids:?}");
    assert!(!ws_ids.contains(&"gamma"), "disabled gamma excluded: {ws_ids:?}");
    assert_eq!(all["result"]["errors"].as_array().unwrap().len(), 0, "no failures expected");
    // Every item carries workspace + qualified_id (R29/R30).
    for g in groups {
        let ws = g["workspace"].as_str().unwrap();
        for item in g["items"].as_array().unwrap() {
            assert_eq!(item["workspace"].as_str().unwrap(), ws);
            assert!(item["qualified_id"].as_str().unwrap().starts_with(&format!("{ws}:")));
        }
    }

    // `get` still rejects `all` (R23): a raw id "all" is not a qualified id and
    // resolving selector "all" is unsupported for get.
    let get_all = client
        .post(format!("{}/tools/get", base))
        .json(&serde_json::json!({ "id": "x", "workspace": "all" }))
        .send()
        .unwrap();
    assert_eq!(
        get_all.json::<serde_json::Value>().unwrap()["error"]["code"],
        "unsupported_workspace_selector"
    );

    // `sources = all` returns one group per enabled workspace.
    let src_all: serde_json::Value = client
        .post(format!("{}/tools/sources", base))
        .json(&serde_json::json!({ "workspace": "all" }))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let src_groups = src_all["result"]["results"].as_array().unwrap();
    let src_ids: Vec<&str> = src_groups.iter().map(|g| g["workspace"].as_str().unwrap()).collect();
    assert!(src_ids.contains(&"alpha") && src_ids.contains(&"beta"));
    assert!(!src_ids.contains(&"gamma"));
```

- [ ] **Step 2: Build the binary and run the integration test**

Run: `cargo build -p context-harness && cargo test -p context-harness --test integration test_multi_workspace_routing -- --nocapture`
Expected: PASS. (The test spawns `ctx serve mcp --workspaces=<registry>`; the binary must be freshly built so the subprocess picks up the Task 5 changes — hence the explicit `cargo build` first.)

- [ ] **Step 3: Commit**

```bash
git add crates/context-harness/tests/integration.rs
git commit -m "test(router): cover workspace=all fan-out for search and sources"
```

---

## Task 7: Documentation — flip Phase 2 to as-built

**Files:**
- Modify: `docs/spec/0014-multi-workspace-mcp-router.md` (status blockquote ~8-14; R37 ~169-172; R64 table ~250-259)
- Modify: `docs/design/0008-multi-workspace-mcp-router.md` (status line 3; status blockquote)
- Modify: `docs/runbook/0018-multi-workspace-mcp-routing.md` (the `all` example that currently says "unsupported")

**Interfaces:** none (docs).

- [ ] **Step 1: SPEC-0014 status blockquote**

Replace the "Deferred: `workspace = "all"` fan-out (Phase 2 ...)" sentence in the implementation-status blockquote so it reads that Phase 2 (`all` fan-out for `search` and `sources`) is now implemented, and only the Phase-3 request-origin / workspace-scoped extensions (requirements 65–82) remain deferred. Update the top `**Status:**` line accordingly.

- [ ] **Step 2: SPEC-0014 R37 — pin the error-entry shape**

Append to requirement 37 a sentence specifying the entry shape and codes:

```
   Each error entry SHALL have the shape `{ "workspace": <id>, "code": <code>,
   "message": <text> }`. A workspace that exceeds the deadline SHALL use code
   `workspace_timeout`; a workspace that is unavailable or whose search fails
   SHALL use code `workspace_unavailable`.
```

- [ ] **Step 3: SPEC-0014 R64 — add the timeout row**

Add a row to the error-code table:

```
   | `workspace_timeout` | A workspace exceeded its per-workspace deadline during an `all` fan-out. |
```

- [ ] **Step 4: DESIGN-0008 status**

Change the status line/blockquote from "Phase 2 design finalized" / "under implementation" to "Phase 1 & 2 implemented", keeping the Phase-3 deferral note.

- [ ] **Step 5: RUNBOOK-0018 `all` example**

Update the `workspace = "all"` example/step: it currently states the selector returns `unsupported_workspace_selector`. Change it to show the grouped fan-out response (`{ "results": [ { "workspace", "items" } … ], "errors": [ … ] }`), note the 5000 ms default deadline and `[defaults].search_deadline_ms` override, and that `get` still rejects `all`.

- [ ] **Step 6: Verify docs build / links (if the repo lints docs)**

Run: `cargo test -p context-harness --test integration 2>&1 | tail -5` (full integration suite still green) and, if a docs-check script exists, run it. Expected: green.

- [ ] **Step 7: Commit**

```bash
git add docs/spec/0014-multi-workspace-mcp-router.md docs/design/0008-multi-workspace-mcp-router.md docs/runbook/0018-multi-workspace-mcp-routing.md
git commit -m "docs: mark multi-workspace router Phase 2 (all fan-out) as implemented"
```

---

## Final Verification

- [ ] Run the whole crate test suite: `cargo build -p context-harness && cargo test -p context-harness`
  Expected: all unit + integration tests pass, including `test_compat_golden_invariant` (proves the additive invariant R15 is intact — compat mode unchanged).
- [ ] Sanity-check the fan-out manually against the dogfood workspace registry if one is configured: `ctx serve mcp --workspaces` then `POST /tools/search {"query":"…","workspace":"all"}` and confirm grouped results + empty `errors`.

## Self-Review Notes (author)

- **Spec coverage:** R32/R33/R34/R35/R36 → Task 4 (`fan_out`) + Task 5 (`search_all`, per-workspace `limit`, grouping); R37 → Task 4 (never-drop) + Task 5 (`errors[]` entries) + Task 6 (assert); R22/R45 (`sources=all`) → Task 5 Step 7 + Task 6; R23 (`get` rejects `all`) → unchanged, asserted in Task 6; R64 timeout code → Task 2 + Task 7. Additive invariant R15 → guarded by leaving compat paths untouched + `test_compat_golden_invariant` in Final Verification.
- **Placeholder scan:** none — every code step shows complete code.
- **Type consistency:** `resolve_all -> (Vec<Arc<WorkspaceRuntime>>, Vec<(String, RouterError)>)`, `fan_out -> Vec<(String, FanOut<T>)>`, `search_group`/`error_entry`/`search_all` signatures are used identically across Tasks 3–6.
