//! Deprecated compatibility names for the pre-profile prompt API.
//!
//! Executable local agents live in [`crate::agent_resource`] and
//! [`crate::agent_runtime`]. Reusable MCP prompt personas now use the canonical
//! [`crate::profiles`] API.

#![allow(unused_imports)]

#[deprecated(note = "use profiles::Profile")]
pub use crate::profiles::Profile as Agent;
#[deprecated(note = "use profiles::ProfileArgument")]
pub use crate::profiles::ProfileArgument as AgentArgument;
#[deprecated(note = "use profiles::ProfileInfo")]
pub use crate::profiles::ProfileInfo as AgentInfo;
#[deprecated(note = "use profiles::ProfilePrompt")]
pub use crate::profiles::ProfilePrompt as AgentPrompt;
#[deprecated(note = "use profiles::ProfileRegistry")]
pub use crate::profiles::ProfileRegistry as AgentRegistry;
pub use crate::profiles::PromptMessage;
#[deprecated(note = "use profiles::TomlProfile")]
pub use crate::profiles::TomlProfile as TomlAgent;
