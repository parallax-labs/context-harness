# Rust Extension Traits — Design Specification

This document specifies the Rust trait system for extending Context Harness
with compiled connectors and tools.

**Status:** Implemented  
**Author:** Parker Jones  
**Created:** 2026-02-22  

---

## 1. Motivation

Lua scripted connectors and tools cover most extension use cases. For some
scenarios, compiled Rust extensions are a better fit:

- **Performance** — connectors scanning millions of files or tools doing
  heavy computation benefit from zero-overhead Rust execution
- **Type safety** — compile-time guarantees prevent entire classes of bugs
- **Deep integration** — Rust connectors can use any crate in the ecosystem
  (database drivers, protocol implementations, etc.)
- **Library authors** — crate authors can publish connectors/tools that
  integrate seamlessly with Context Harness

### Target Audience

| Audience | Use Case |
|----------|----------|
| Power users | Custom connector for proprietary system |
| Library authors | Published crate implementing `Connector` |
| Context Harness contributors | Built-in connectors eventually implement traits |
| CI/CD pipelines | Compiled tools with no scripting overhead |

---

## 2. Trait Definitions

### 2.1 `Connector` Trait

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    /// Returns the connector's display name.
    fn name(&self) -> &str;

    /// Returns a one-line description of what this connector does.
    fn description(&self) -> &str;

    /// Scan the data source and return all items to ingest.
    async fn scan(&self) -> Result<Vec<SourceItem>>;
}
```

### 2.2 `Tool` Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the tool's name (used as route path).
    fn name(&self) -> &str;

    /// Returns a one-line description for agent discovery.
    fn description(&self) -> &str;

    /// Returns the OpenAI function-calling JSON Schema for parameters.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with validated parameters.
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value>;
}
```

---

## 3. ToolContext

The `ToolContext` provides tools with access to the Context Harness
knowledge base — the same core functions used by the CLI and server.

```rust
pub struct ToolContext {
    config: Arc<Config>,
}

impl ToolContext {
    pub fn new(config: Arc<Config>) -> Self;
    pub async fn search(&self, query: &str, opts: SearchOptions) -> Result<Vec<SearchResultItem>>;
    pub async fn get(&self, id: &str) -> Result<DocumentResponse>;
    pub fn sources(&self) -> Result<Vec<SourceStatus>>;
}

#[derive(Debug, Default)]
pub struct SearchOptions {
    pub mode: Option<String>,
    pub limit: Option<i64>,
    pub source: Option<String>,
}
```

---

## 4. Registries

### 4.1 `ConnectorRegistry`

```rust
pub struct ConnectorRegistry {
    connectors: Vec<Box<dyn Connector>>,
}

impl ConnectorRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, connector: Box<dyn Connector>);
    pub fn connectors(&self) -> &[Box<dyn Connector>];
    pub fn is_empty(&self) -> bool;
}
```

Custom connectors are synced with `ctx sync custom:<name>` or included
in `ctx sync all`.

### 4.2 `ToolRegistry`

```rust
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, tool: Box<dyn Tool>);
    pub fn tools(&self) -> &[Box<dyn Tool>];
    pub fn is_empty(&self) -> bool;
}
```

Custom tools appear in `GET /tools/list` and can be called via
`POST /tools/{name}`.

---

## 5. Server Integration

### 5.1 `run_server_with_extensions`

```rust
pub async fn run_server_with_extensions(
    config: &Config,
    custom_tools: Arc<ToolRegistry>,
) -> anyhow::Result<()>;
```

Tools are resolved in priority order:

1. **Custom Rust tools** — from `ToolRegistry`
2. **Lua tools** — from `[tools.script.*]` config
3. **Built-in tools** — `search`, `get`, `sources`

### 5.2 Updated AppState

```rust
struct AppState {
    config: Arc<Config>,
    lua_tools: Arc<Vec<ToolDefinition>>,
    custom_tools: Arc<ToolRegistry>,
}
```

---

## 6. Sync Integration

### 6.1 `run_sync_with_extensions`

```rust
pub async fn run_sync_with_extensions(
    config: &Config,
    connector: &str,
    full: bool,
    dry_run: bool,
    since: Option<String>,
    until: Option<String>,
    limit: Option<usize>,
    custom_connectors: &ConnectorRegistry,
) -> Result<()>;
```

Custom connectors are synced with:

```bash
ctx sync custom:database    # sync a specific custom connector
ctx sync all                # syncs built-in + script + custom connectors
```

---

## 7. Example: Custom Binary

```rust
// my-harness/src/main.rs
use context_harness::config;
use context_harness::server::run_server_with_extensions;
use context_harness::traits::{ConnectorRegistry, ToolRegistry};
use std::sync::Arc;

mod my_connector;
mod my_tool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::load_config(&"config/ctx.toml".into())?;

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(my_tool::HealthCheckTool));

    run_server_with_extensions(&cfg, Arc::new(tools)).await
}
```

### Minimal Dependency Set

Users implementing traits only need:

```toml
[dependencies]
context-harness = "0.1"
async-trait = "0.1"
anyhow = "1"
serde_json = "1"
chrono = "0.4"
```

---

## 8. Future Extensions

### 8.1 Built-in Connector Adapters

Eventually, the built-in connectors could also implement the `Connector`
trait, providing a fully trait-based architecture:

```rust
pub struct FilesystemConnector { name: String, config: FilesystemConnectorConfig }

#[async_trait]
impl Connector for FilesystemConnector {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "Walk local directories" }
    async fn scan(&self) -> Result<Vec<SourceItem>> {
        connector_fs::scan_filesystem(&self.name, &self.config)
    }
}
```

### 8.2 Dynamic Loading (Plugins)

A future version could support dynamic loading of compiled extensions
as shared libraries (`.so`/`.dylib`/`.dll`) via `libloading`.

### 8.3 Default Method Evolution

Traits may gain methods with default implementations in minor versions:

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn scan(&self) -> Result<Vec<SourceItem>>;

    // Added in v0.2 — default preserves backwards compat
    fn supports_incremental(&self) -> bool { false }
}
```

---

## 9. Stability

The public contract is defined by:
- `RUST_TRAITS.md` (this document)
- The `context_harness::traits` module

Changes to trait signatures or registration APIs constitute breaking
changes and require a major version bump.
