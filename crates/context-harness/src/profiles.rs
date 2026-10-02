//! Profile system for reusable MCP prompts and personas.
//!
//! Profiles are named personas that combine a system prompt, scoped tools, and
//! optional dynamic context injection. They enable "assume a role" workflows
//! in Cursor, Claude Desktop, and other MCP clients.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │            ProfileRegistry                 │
//! │  ┌─────────┐ ┌─────────┐ ┌────────────┐ │
//! │  │  TOML   │ │  Lua    │ │  Custom    │ │
//! │  │ Inline  │ │ Script  │ │ (Rust)     │ │
//! │  └─────────┘ └─────────┘ └────────────┘ │
//! └──────────────┬───────────────────────────┘
//!                ▼
//!   GET /profiles/list  ·  POST /profiles/{name}/prompt
//! ```
//!
//! # Profile Sources
//!
//! | Source | Config Key | Struct |
//! |--------|------------|--------|
//! | Inline TOML | `[profiles.inline.<name>]` | [`TomlProfile`] |
//! | Lua script | `[profiles.script.<name>]` | `LuaProfileAdapter` (in [`crate::profile_script`]) |
//! | Custom Rust | `registry.register(...)` | User-defined [`Profile`] impl |
//!
//! # Usage
//!
//! ```rust
//! use context_harness::profiles::{ProfileRegistry, TomlProfile};
//!
//! let mut profiles = ProfileRegistry::new();
//! profiles.register(Box::new(TomlProfile::new(
//!     "reviewer".to_string(),
//!     "Reviews code against conventions".to_string(),
//!     vec!["search".to_string(), "get".to_string()],
//!     "You are a senior code reviewer.".to_string(),
//! )));
//! ```
//!
//! See the profiles documentation for the full specification.

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::traits::ToolContext;

// ═══════════════════════════════════════════════════════════════════════
// Profile Trait
// ═══════════════════════════════════════════════════════════════════════

/// A reusable persona that provides a system prompt and tool scoping.
///
/// Implement this trait to create a custom profile in Rust. Profiles are
/// registered in a [`ProfileRegistry`] and exposed via `GET /profiles/list`
/// for discovery and `POST /profiles/{name}/prompt` for resolution.
///
/// # Lifecycle
///
/// 1. The profile is registered via [`ProfileRegistry::register`].
/// 2. At discovery time, [`name`](Profile::name), [`description`](Profile::description),
///    [`tools`](Profile::tools), and [`arguments`](Profile::arguments) are called.
/// 3. When a user selects the profile, [`resolve`](Profile::resolve) is called
///    with any provided arguments and a [`ToolContext`] for KB access.
///
/// # Example
///
/// ```rust
/// use async_trait::async_trait;
/// use anyhow::Result;
/// use serde_json::{json, Value};
/// use context_harness::profiles::{Profile, ProfilePrompt, ProfileArgument};
/// use context_harness::traits::ToolContext;
///
/// pub struct ArchitectProfile;
///
/// #[async_trait]
/// impl Profile for ArchitectProfile {
///     fn name(&self) -> &str { "architect" }
///     fn description(&self) -> &str { "Answers architecture questions" }
///     fn tools(&self) -> Vec<String> { vec!["search".into(), "get".into()] }
///
///     async fn resolve(&self, _args: Value, _ctx: &ToolContext) -> Result<ProfilePrompt> {
///         Ok(ProfilePrompt {
///             system: "You are a software architect.".to_string(),
///             tools: self.tools(),
///             messages: vec![],
///         })
///     }
/// }
/// ```
#[async_trait]
pub trait Profile: Send + Sync {
    /// Returns the profile's unique name (URL-safe, e.g. `"code-reviewer"`).
    fn name(&self) -> &str;

    /// Returns a one-line description for profile discovery.
    fn description(&self) -> &str;

    /// Returns the list of tool names this profile exposes.
    fn tools(&self) -> Vec<String>;

    /// Returns the profile's source type: `"toml"`, `"lua"`, or `"rust"`.
    fn source(&self) -> &str {
        "rust"
    }

    /// Returns the arguments this profile accepts (may be empty).
    ///
    /// Arguments are shown to the user in MCP prompt selection UIs
    /// and passed to [`resolve`](Profile::resolve) as a JSON object.
    fn arguments(&self) -> Vec<ProfileArgument> {
        vec![]
    }

    /// Resolve the profile's prompt, optionally using the [`ToolContext`]
    /// for dynamic context injection (e.g., pre-searching the KB).
    ///
    /// # Arguments
    ///
    /// * `args` — User-provided argument values (JSON object).
    /// * `ctx` — Bridge to the Context Harness knowledge base.
    ///
    /// # Returns
    ///
    /// A [`ProfilePrompt`] containing the system prompt, tool list,
    /// and optional pre-injected messages.
    async fn resolve(&self, args: Value, ctx: &ToolContext) -> Result<ProfilePrompt>;
}

// ═══════════════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════════════

/// An argument that an profile accepts.
///
/// Arguments are shown in MCP prompt selection UIs. When the user
/// selects a profile, argument values are collected and passed to
/// [`Profile::resolve`].
#[derive(Debug, Clone, Serialize)]
pub struct ProfileArgument {
    /// Argument name (e.g. `"service"`).
    pub name: String,
    /// Description shown to the user.
    pub description: String,
    /// Whether this argument must be provided.
    pub required: bool,
}

/// A resolved profile prompt ready for the LLM.
///
/// Returned by [`Profile::resolve`]. The client (Cursor, Claude, etc.)
/// uses this to configure the LLM conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ProfilePrompt {
    /// The system prompt text.
    pub system: String,
    /// Which tools should be visible for this profile.
    pub tools: Vec<String>,
    /// Optional additional messages to inject at conversation start.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<PromptMessage>,
}

/// A message to inject into the conversation.
///
/// Used by profiles that want to pre-populate context (e.g., pre-fetched
/// search results) or provide an initial assistant greeting.
#[derive(Debug, Clone, Serialize)]
pub struct PromptMessage {
    /// Message role: `"user"`, `"assistant"`, or `"system"`.
    pub role: String,
    /// Message content.
    pub content: String,
}

/// Serializable profile info for the `/profiles/list` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileInfo {
    /// Profile name (used as URL path parameter).
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Tools this profile uses.
    pub tools: Vec<String>,
    /// Source type: `"toml"`, `"lua"`, or `"rust"`.
    pub source: String,
    /// Arguments this profile accepts.
    pub arguments: Vec<ProfileArgument>,
}

// ═══════════════════════════════════════════════════════════════════════
// TomlProfile
// ═══════════════════════════════════════════════════════════════════════

/// An profile defined inline in TOML configuration.
///
/// The simplest profile type — has a static system prompt and fixed tool
/// list. No dynamic context injection or arguments.
///
/// Created automatically by [`ProfileRegistry::from_config`] for each
/// `[profiles.inline.<name>]` entry.
pub struct TomlProfile {
    name: String,
    description: String,
    tools: Vec<String>,
    system_prompt: String,
}

impl TomlProfile {
    /// Create a new inline TOML profile.
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
impl Profile for TomlProfile {
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

    async fn resolve(&self, _args: Value, _ctx: &ToolContext) -> Result<ProfilePrompt> {
        Ok(ProfilePrompt {
            system: self.system_prompt.clone(),
            tools: self.tools.clone(),
            messages: vec![],
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ProfileRegistry
// ═══════════════════════════════════════════════════════════════════════

/// Registry for profiles (TOML, Lua, and custom Rust).
///
/// Use [`ProfileRegistry::from_config`] to create a registry pre-loaded
/// with all profiles from the config file, then optionally call
/// [`register`](ProfileRegistry::register) to add custom Rust profiles.
///
/// # Example
///
/// ```rust
/// use context_harness::profiles::ProfileRegistry;
///
/// let mut profiles = ProfileRegistry::new();
/// // profiles.register(Box::new(MyProfile::new()));
/// ```
pub struct ProfileRegistry {
    profiles: Vec<Box<dyn Profile>>,
}

impl ProfileRegistry {
    /// Create an empty profile registry.
    pub fn new() -> Self {
        Self {
            profiles: Vec::new(),
        }
    }

    /// Create a registry pre-loaded with all profiles from config.
    ///
    /// Loads inline TOML profiles from `[profiles.inline.*]` entries.
    /// Lua script profiles from `[profiles.script.*]` are loaded separately
    /// via [`crate::profile_script::load_profile_definitions`].
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut registry = Self::new();

        // Load inline TOML profiles
        for (name, cfg) in &config.profiles.inline {
            registry.register(Box::new(TomlProfile::new(
                name.clone(),
                cfg.description.clone(),
                cfg.tools.clone(),
                cfg.system_prompt.clone(),
            )));
        }

        // Lua profiles are loaded in profile_script::load_profile_definitions
        // and registered by the caller (server.rs / main.rs).

        Ok(registry)
    }

    /// Register a profile.
    pub fn register(&mut self, profile: Box<dyn Profile>) {
        self.profiles.push(profile);
    }

    /// Get all registered profiles.
    pub fn profiles(&self) -> &[Box<dyn Profile>] {
        &self.profiles
    }

    /// Find a profile by name.
    pub fn find(&self, name: &str) -> Option<&dyn Profile> {
        self.profiles
            .iter()
            .find(|a| a.name() == name)
            .map(|a| a.as_ref())
    }

    /// Check if the registry is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Return the count of registered profiles.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::new()
    }
}
