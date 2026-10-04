//! Per-run external tool discovery. Even initialization starts an unsandboxed
//! process, so it passes through durable authorization before connecting.
use super::*;
use crate::agent_resource::Capability;
use std::collections::BTreeSet;

pub(super) const CAPABILITIES: [Capability; 2] =
    [Capability::ProcessExecute, Capability::ExternalSideEffect];

impl AgentRuntime {
    pub(super) async fn external_tools(
        &self,
        id: &str,
        resource: &LoadedAgentResource,
    ) -> Result<(ToolRegistry, Vec<mcp_client::Session>)> {
        let mut servers = BTreeSet::new();
        // Validate every declaration before starting any process, including known
        // builtins, so a denied or missing declaration cannot trigger startup.
        for name in &resource.definition.agent.tools {
            let implementation = self
                .tool_bindings
                .get(name)
                .map(|binding| binding.definition.tool.implementation.as_str())
                .unwrap_or(name);
            if let Some((server, tool)) = tool_binding::mcp_reference(implementation) {
                ensure!(!tool.is_empty(), "MCP tool name is empty");
                let config = self
                    .config
                    .mcp_servers
                    .get(server)
                    .context("MCP server is not configured")?;
                config.validate(server)?;
                ensure!(
                    self.policy
                        .authorize(&resource.definition.agent.permissions, &CAPABILITIES)
                        != Authorization::Denied,
                    "MCP tools require process_execute and external_side_effect permissions"
                );
                servers.insert(server.to_owned());
            } else {
                let tool = self
                    .tools
                    .find(name)
                    .context("unsupported runtime tool declaration")?;
                if matches!(tool.runtime_dispatch(), ToolRuntimeDispatch::RunControl(_)) {
                    continue;
                }
                let capabilities = tool
                    .capabilities()
                    .context("tool capability metadata is unavailable")?;
                ensure!(
                    self.policy
                        .authorize(&resource.definition.agent.permissions, &capabilities)
                        != Authorization::Denied,
                    "tool capability is not permitted by agent and host policy"
                );
            }
        }
        ensure!(
            servers.len() <= 8,
            "at most 8 MCP servers may be used in one run"
        );
        let mut registry = ToolRegistry::new();
        let mut sessions = Vec::new();
        for name in servers {
            let config = &self.config.mcp_servers[&name];
            let call_id = format!("mcp-start-{}", uuid::Uuid::new_v4());
            let tool = format!("runtime.mcp.start.{name}");
            let arguments =
                json!({"server":name,"command":config.command,"args":config.args,"cwd":self.root});
            self.store
                .request_tool(id, &call_id, &tool, &arguments)
                .await?;
            let authorization = self
                .policy
                .authorize(&resource.definition.agent.permissions, &CAPABILITIES);
            self.approve_invocation(id, &call_id, &tool, &arguments, authorization)
                .await?;
            self.store.start_tool(id, &call_id).await?;
            let result = mcp_client::connect(&name, config, &self.root).await;
            match result {
                Ok(session) => {
                    if session.register(&mut registry).is_err() {
                        self.store
                            .finish_tool(
                                id,
                                &call_id,
                                ToolOutcome::Failed("MCP discovery failed".into()),
                            )
                            .await?;
                        anyhow::bail!("MCP discovery failed");
                    }
                    for binding in resource
                        .definition
                        .agent
                        .tools
                        .iter()
                        .filter_map(|tool| self.tool_bindings.get(tool))
                        .filter(|binding| {
                            tool_binding::mcp_reference(&binding.definition.tool.implementation)
                                .is_some_and(|(binding_server, _)| binding_server == name)
                        })
                    {
                        if session
                            .register_alias(binding, config, &mut registry)
                            .is_err()
                        {
                            self.store
                                .finish_tool(
                                    id,
                                    &call_id,
                                    ToolOutcome::Failed(
                                        "MCP alias schema validation failed".into(),
                                    ),
                                )
                                .await?;
                            anyhow::bail!("MCP alias schema validation failed");
                        }
                    }
                    self.store
                        .finish_tool(
                            id,
                            &call_id,
                            ToolOutcome::Completed(json!({"server":name,"connected":true})),
                        )
                        .await?;
                    sessions.push(session);
                }
                Err(_) => {
                    self.store
                        .finish_tool(
                            id,
                            &call_id,
                            ToolOutcome::Failed("MCP connection or discovery failed".into()),
                        )
                        .await?;
                    anyhow::bail!("MCP connection or discovery failed");
                }
            }
        }
        Ok((registry, sessions))
    }

    pub(super) async fn approve_invocation(
        &self,
        id: &str,
        call_id: &str,
        tool: &str,
        arguments: &serde_json::Value,
        authorization: Authorization,
    ) -> Result<()> {
        if authorization == Authorization::Denied {
            self.store
                .finish_tool(
                    id,
                    call_id,
                    ToolOutcome::Denied("tool capability denied".into()),
                )
                .await?;
            anyhow::bail!("tool capability denied");
        }
        if let Authorization::Approval(capabilities) = authorization {
            self.store
                .request_approval(id, call_id, &capabilities)
                .await?;
            let granted = self
                .approvals
                .approve(&ApprovalRequest {
                    run_id: id.into(),
                    call_id: call_id.into(),
                    tool: tool.into(),
                    capabilities,
                    arguments: arguments.clone(),
                })
                .await;
            self.store.decide_approval(id, call_id, granted).await?;
            if !granted {
                self.store
                    .finish_tool(id, call_id, ToolOutcome::Denied("approval denied".into()))
                    .await?;
                anyhow::bail!("tool approval denied");
            }
        }
        Ok(())
    }
}
