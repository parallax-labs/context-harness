//! Deprecated compatibility module for Lua scripted profiles.

#![allow(deprecated, unused_imports)]

#[deprecated(note = "use profile_script")]
pub use crate::profile_script::*;
pub use crate::profile_script::{
    list_profiles as list_agents, load_profile_definitions as load_agent_definitions,
    load_single_profile as load_single_agent, resolve_profile as resolve_agent,
    scaffold_profile as scaffold_agent, test_profile as test_agent,
    LuaProfileAdapter as LuaAgentAdapter, ProfileDefinition as AgentDefinition,
};
