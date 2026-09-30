//! Host-owned permission ceilings and per-invocation approval. Agent resources
//! can restrict this policy, never widen it.
use crate::agent_resource::{Capability, Permissions};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct RuntimePolicy {
    pub allow: Vec<Capability>,
    pub require_approval: Vec<Capability>,
}
impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            allow: vec![Capability::ReadOnly],
            require_approval: vec![Capability::WorkspaceWrite, Capability::ProcessExecute],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authorization {
    Denied,
    Allowed,
    Approval(Vec<Capability>),
}
impl RuntimePolicy {
    pub fn authorize(&self, agent: &Permissions, capabilities: &[Capability]) -> Authorization {
        if capabilities.is_empty() {
            return Authorization::Denied;
        }
        let allowed = agent.allowed();
        let mut approval = Vec::new();
        for capability in capabilities {
            if (!self.allow.contains(capability) && !self.require_approval.contains(capability))
                || (!allowed.contains(capability) && !agent.require_approval.contains(capability))
            {
                return Authorization::Denied;
            }
            if (self.require_approval.contains(capability)
                || agent.require_approval.contains(capability))
                && !approval.contains(capability)
            {
                approval.push(*capability);
            }
        }
        if approval.is_empty() {
            Authorization::Allowed
        } else {
            Authorization::Approval(approval)
        }
    }
}

/// Approval applies only to this exact invocation. Implementations must not
/// interpret model text as user consent. Dropping the future must stop waiting.
pub struct ApprovalRequest {
    pub run_id: String,
    pub call_id: String,
    pub tool: String,
    pub capabilities: Vec<Capability>,
    pub arguments: Value,
}
#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    async fn approve(&self, request: &ApprovalRequest) -> bool;
}
pub struct DenyApprovals;
#[async_trait]
impl ApprovalHandler for DenyApprovals {
    async fn approve(&self, _: &ApprovalRequest) -> bool {
        false
    }
}
