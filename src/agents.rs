//! Agent system for MCP prompts and personas.
//!
//! Agents are named personas that combine a system prompt, scoped tools, and
//! optional dynamic context injection. They enable "assume a role" workflows
//! in Cursor, Claude Desktop, and other MCP clients.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │            AgentRegistry                 │
//! │  ┌─────────┐ ┌─────────┐ ┌────────────┐ │
//! │  │  TOML   │ │  Lua    │ │  Custom    │ │
//! │  │ Inline  │ │ Script  │ │ (Rust)     │ │
//! │  └─────────┘ └─────────┘ └────────────┘ │
//! └──────────────┬───────────────────────────┘
//!                ▼
//!   GET /agents/list  ·  POST /agents/{name}/prompt
//! ```
//!
//! # Agent Sources
//!
//! | Source | Config Key | Struct |
//! |--------|------------|--------|
//! | Inline TOML | `[agents.inline.<name>]` | [`TomlAgent`] |
//! | Lua script | `[agents.script.<name>]` | `LuaAgentAdapter` (in [`crate::agent_script`]) |
//! | Custom Rust | `registry.register(...)` | User-defined [`Agent`] impl |
//!
//! # Usage
//!
//! ```rust
//! use context_harness::agents::{AgentRegistry, TomlAgent};
//!
//! let mut agents = AgentRegistry::new();
//! agents.register(Box::new(TomlAgent::new(
//!     "reviewer".to_string(),
//!     "Reviews code against conventions".to_string(),
//!     vec!["search".to_string(), "get".to_string()],
//!     "You are a senior code reviewer.".to_string(),
//! )));
//! ```
//!
//! See `docs/AGENTS.md` for the full specification.

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::traits::ToolContext;

// ═══════════════════════════════════════════════════════════════════════
// Agent Trait
// ═══════════════════════════════════════════════════════════════════════

/// An agent persona that provides a system prompt and tool scoping.
///
/// Implement this trait to create a custom agent in Rust. Agents are
/// registered in an [`AgentRegistry`] and exposed via `GET /agents/list`
/// for discovery and `POST /agents/{name}/prompt` for resolution.
///
/// # Lifecycle
///
/// 1. The agent is registered via [`AgentRegistry::register`].
/// 2. At discovery time, [`name`](Agent::name), [`description`](Agent::description),
///    [`tools`](Agent::tools), and [`arguments`](Agent::arguments) are called.
/// 3. When a user selects the agent, [`resolve`](Agent::resolve) is called
///    with any provided arguments and a [`ToolContext`] for KB access.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use anyhow::Result;
/// use serde_json::{json, Value};
/// use context_harness::agents::{Agent, AgentPrompt, AgentArgument};
/// use context_harness::traits::ToolContext;
///
/// pub struct ArchitectAgent;
///
/// #[async_trait]
/// impl Agent for ArchitectAgent {
///     fn name(&self) -> &str { "architect" }
///     fn description(&self) -> &str { "Answers architecture questions" }
///     fn tools(&self) -> Vec<String> { vec!["search".into(), "get".into()] }
///
///     async fn resolve(&self, _args: Value, _ctx: &ToolContext) -> Result<AgentPrompt> {
///         Ok(AgentPrompt {
///             system: "You are a software architect.".to_string(),
///             tools: self.tools(),
///             messages: vec![],
///         })
///     }
/// }
/// ```
#[async_trait]
pub trait Agent: Send + Sync {
    /// Returns the agent's unique name (URL-safe, e.g. `"code-reviewer"`).
    fn name(&self) -> &str;

    /// Returns a one-line description for agent discovery.
    fn description(&self) -> &str;

    /// Returns the list of tool names this agent exposes.
    fn tools(&self) -> Vec<String>;

    /// Returns the agent's source type: `"toml"`, `"lua"`, or `"rust"`.
    fn source(&self) -> &str {
        "rust"
    }

    /// Returns the arguments this agent accepts (may be empty).
    ///
    /// Arguments are shown to the user in MCP prompt selection UIs
    /// and passed to [`resolve`](Agent::resolve) as a JSON object.
    fn arguments(&self) -> Vec<AgentArgument> {
        vec![]
    }

    /// Resolve the agent's prompt, optionally using the [`ToolContext`]
    /// for dynamic context injection (e.g., pre-searching the KB).
    ///
    /// # Arguments
    ///
    /// * `args` — User-provided argument values (JSON object).
    /// * `ctx` — Bridge to the Context Harness knowledge base.
    ///
    /// # Returns
    ///
    /// An [`AgentPrompt`] containing the system prompt, tool list,
    /// and optional pre-injected messages.
    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<AgentPrompt>;
}

// ═══════════════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════════════

/// An argument that an agent accepts.
///
/// Arguments are shown in MCP prompt selection UIs. When the user
/// selects an agent, argument values are collected and passed to
/// [`Agent::resolve`].
#[derive(Debug, Clone, Serialize)]
pub struct AgentArgument {
    /// Argument name (e.g. `"service"`).
    pub name: String,
    /// Description shown to the user.
    pub description: String,
    /// Whether this argument must be provided.
    pub required: bool,
}

/// A resolved agent prompt ready for the LLM.
///
/// Returned by [`Agent::resolve`]. The client (Cursor, Claude, etc.)
/// uses this to configure the LLM conversation.
#[derive(Debug, Clone, Serialize)]
pub struct AgentPrompt {
    /// The system prompt text.
    pub system: String,
    /// Which tools should be visible for this agent.
    pub tools: Vec<String>,
    /// Optional additional messages to inject at conversation start.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<PromptMessage>,
}

/// A message to inject into the conversation.
///
/// Used by agents that want to pre-populate context (e.g., pre-fetched
/// search results) or provide an initial assistant greeting.
#[derive(Debug, Clone, Serialize)]
pub struct PromptMessage {
    /// Message role: `"user"`, `"assistant"`, or `"system"`.
    pub role: String,
    /// Message content.
    pub content: String,
}

/// Serializable agent info for the `/agents/list` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    /// Agent name (used as URL path parameter).
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Tools this agent uses.
    pub tools: Vec<String>,
    /// Source type: `"toml"`, `"lua"`, or `"rust"`.
    pub source: String,
    /// Arguments this agent accepts.
    pub arguments: Vec<AgentArgument>,
}

// ═══════════════════════════════════════════════════════════════════════
// TomlAgent
// ═══════════════════════════════════════════════════════════════════════

/// An agent defined inline in TOML configuration.
///
/// The simplest agent type — has a static system prompt and fixed tool
/// list. No dynamic context injection or arguments.
///
/// Created automatically by [`AgentRegistry::from_config`] for each
/// `[agents.inline.<name>]` entry.
pub struct TomlAgent {
    name: String,
    description: String,
    tools: Vec<String>,
    system_prompt: String,
}

impl TomlAgent {
    /// Create a new inline TOML agent.
    pub fn new(
        name: String,
        description: String,
        tools: Vec<String>,
        system_prompt: String,
    ) -> Self {
        Self {
            name,
            description,
            tools,
            system_prompt,
        }
    }
}

#[async_trait]
impl Agent for TomlAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn tools(&self) -> Vec<String> {
        self.tools.clone()
    }

    fn source(&self) -> &str {
        "toml"
    }

    async fn resolve(&self, _args: Value, _ctx: &ToolContext) -> Result<AgentPrompt> {
        Ok(AgentPrompt {
            system: self.system_prompt.clone(),
            tools: self.tools.clone(),
            messages: vec![],
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// AgentRegistry
// ═══════════════════════════════════════════════════════════════════════

/// Registry for agents (TOML, Lua, and custom Rust).
///
/// Use [`AgentRegistry::from_config`] to create a registry pre-loaded
/// with all agents from the config file, then optionally call
/// [`register`](AgentRegistry::register) to add custom Rust agents.
///
/// # Example
///
/// ```rust
/// use context_harness::agents::AgentRegistry;
///
/// let mut agents = AgentRegistry::new();
/// // agents.register(Box::new(MyAgent::new()));
/// ```
pub struct AgentRegistry {
    agents: Vec<Box<dyn Agent>>,
}

impl AgentRegistry {
    /// Create an empty agent registry.
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    /// Create a registry pre-loaded with all agents from config.
    ///
    /// Loads inline TOML agents from `[agents.inline.*]` entries.
    /// Lua script agents from `[agents.script.*]` are loaded separately
    /// via [`crate::agent_script::load_agent_definitions`].
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut registry = Self::new();

        // Load inline TOML agents
        for (name, cfg) in &config.agents.inline {
            registry.register(Box::new(TomlAgent::new(
                name.clone(),
                cfg.description.clone(),
                cfg.tools.clone(),
                cfg.system_prompt.clone(),
            )));
        }

        // Lua agents are loaded in agent_script::load_agent_definitions
        // and registered by the caller (server.rs / main.rs).

        Ok(registry)
    }

    /// Register an agent.
    pub fn register(&mut self, agent: Box<dyn Agent>) {
        self.agents.push(agent);
    }

    /// Get all registered agents.
    pub fn agents(&self) -> &[Box<dyn Agent>] {
        &self.agents
    }

    /// Find an agent by name.
    pub fn find(&self, name: &str) -> Option<&dyn Agent> {
        self.agents
            .iter()
            .find(|a| a.name() == name)
            .map(|a| a.as_ref())
    }

    /// Check if the registry is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Return the count of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
