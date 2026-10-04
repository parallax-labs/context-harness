use crate::{
    agent_store::{BlockedReason, NeedsUserInputReason, RunOutcome},
    traits::{RunControlKind, Tool, ToolContext, ToolRuntimeDispatch},
};
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) const BLOCKED: &str = "run.blocked";
pub(super) const REQUEST_USER_INPUT: &str = "run.request_user_input";

pub(super) fn is_reserved(name: &str) -> bool {
    matches!(name, BLOCKED | REQUEST_USER_INPUT)
}

pub(super) fn register(registry: &mut crate::traits::ToolRegistry) {
    registry.register(Box::new(ControlTool(RunControlKind::Blocked)));
    registry.register(Box::new(ControlTool(RunControlKind::RequestUserInput)));
}

pub(super) fn outcome(kind: RunControlKind, arguments: Value) -> Result<RunOutcome> {
    match kind {
        RunControlKind::Blocked => {
            let args: BlockedArgs = serde_json::from_value(arguments)?;
            ensure!(
                !args.message.trim().is_empty() && args.message.len() <= 8192,
                "invalid blocked message"
            );
            Ok(RunOutcome::Blocked {
                code: args.code,
                message: args.message,
            })
        }
        RunControlKind::RequestUserInput => {
            let args: UserInputArgs = serde_json::from_value(arguments)?;
            ensure!(
                !args.question.trim().is_empty() && args.question.len() <= 8192,
                "invalid user-input question"
            );
            Ok(RunOutcome::NeedsUserInput {
                code: args.code,
                question: args.question,
            })
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockedArgs {
    code: BlockedReason,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserInputArgs {
    code: NeedsUserInputReason,
    question: String,
}

struct ControlTool(RunControlKind);

#[async_trait]
impl Tool for ControlTool {
    fn capabilities(&self) -> Option<Vec<crate::agent_resource::Capability>> {
        Some(Vec::new())
    }

    fn runtime_dispatch(&self) -> ToolRuntimeDispatch {
        ToolRuntimeDispatch::RunControl(self.0)
    }

    fn name(&self) -> &str {
        match self.0 {
            RunControlKind::Blocked => BLOCKED,
            RunControlKind::RequestUserInput => REQUEST_USER_INPUT,
        }
    }

    fn description(&self) -> &str {
        match self.0 {
            RunControlKind::Blocked => "Suspend because an external condition prevents progress",
            RunControlKind::RequestUserInput => "Suspend with one concrete question for the user",
        }
    }

    fn parameters_schema(&self) -> Value {
        match self.0 {
            RunControlKind::Blocked => json!({
                "type":"object", "additionalProperties":false,
                "properties":{
                    "code":{"type":"string","enum":["external_dependency","environment_unavailable","policy_restriction","resource_unavailable"]},
                    "message":{"type":"string","minLength":1,"maxLength":8192}
                }, "required":["code","message"]
            }),
            RunControlKind::RequestUserInput => json!({
                "type":"object", "additionalProperties":false,
                "properties":{
                    "code":{"type":"string","enum":["decision_required","information_required"]},
                    "question":{"type":"string","minLength":1,"maxLength":8192}
                }, "required":["code","question"]
            }),
        }
    }

    fn validate_arguments(&self, params: &Value) -> Result<()> {
        outcome(self.0, params.clone()).map(|_| ())
    }

    async fn execute(&self, _params: Value, _ctx: &ToolContext) -> Result<Value> {
        bail!("run controls are handled by the agent runtime")
    }
}
