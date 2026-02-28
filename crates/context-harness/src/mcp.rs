//! MCP JSON-RPC protocol bridge.
//!
//! Adapts the existing [`ToolRegistry`] / [`AgentRegistry`] and REST API
//! into a proper MCP Streamable HTTP endpoint that Cursor and other MCP
//! clients can connect to using the standard JSON-RPC protocol.
//!
//! * **Tools** are exposed as MCP tools via `list_tools` / `call_tool`.
//! * **Agents** are exposed as MCP prompts via `list_prompts` / `get_prompt`.

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::*;
use rmcp::{ErrorData as McpError, ServerHandler};

use crate::agents::AgentRegistry;
use crate::config::Config;
use crate::traits::{ToolContext, ToolRegistry};

/// Bridges the existing registries to the MCP JSON-RPC protocol.
///
/// Each MCP session receives a clone of this struct (everything is
/// behind `Arc`), so all sessions share the same tool set and agents.
#[derive(Clone)]
pub struct McpBridge {
    config: Arc<Config>,
    tools: Arc<ToolRegistry>,
    extra_tools: Arc<ToolRegistry>,
    agents: Arc<AgentRegistry>,
    extra_agents: Arc<AgentRegistry>,
}

impl McpBridge {
    pub fn new(
        config: Arc<Config>,
        tools: Arc<ToolRegistry>,
        extra_tools: Arc<ToolRegistry>,
        agents: Arc<AgentRegistry>,
        extra_agents: Arc<AgentRegistry>,
    ) -> Self {
        Self {
            config,
            tools,
            extra_tools,
            agents,
            extra_agents,
        }
    }

    fn find_tool(&self, name: &str) -> Option<&dyn crate::traits::Tool> {
        self.tools
            .find(name)
            .or_else(|| self.extra_tools.find(name))
    }

    fn find_agent(&self, name: &str) -> Option<&dyn crate::agents::Agent> {
        self.agents
            .find(name)
            .or_else(|| self.extra_agents.find(name))
    }

    /// Convert a context-harness tool into an rmcp `Tool` descriptor.
    fn to_mcp_tool(tool: &dyn crate::traits::Tool) -> Tool {
        let schema_value = tool.parameters_schema();
        let input_schema: Arc<serde_json::Map<String, serde_json::Value>> = match schema_value {
            serde_json::Value::Object(map) => Arc::new(map),
            _ => Arc::new(serde_json::Map::new()),
        };

        Tool {
            name: Cow::Owned(tool.name().to_string()),
            title: None,
            description: Some(Cow::Owned(tool.description().to_string())),
            input_schema,
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true)),
            execution: None,
            icons: None,
            meta: None,
        }
    }

    /// Convert a context-harness agent into an rmcp `Prompt` descriptor.
    fn to_mcp_prompt(agent: &dyn crate::agents::Agent) -> Prompt {
        let arguments: Option<Vec<PromptArgument>> = {
            let args = agent.arguments();
            if args.is_empty() {
                None
            } else {
                Some(
                    args.into_iter()
                        .map(|a| PromptArgument {
                            name: a.name,
                            title: None,
                            description: Some(a.description),
                            required: Some(a.required),
                        })
                        .collect(),
                )
            }
        };

        Prompt {
            name: agent.name().to_string(),
            title: None,
            description: Some(agent.description().to_string()),
            arguments,
            icons: None,
            meta: None,
        }
    }
}

impl ServerHandler for McpBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
            server_info: Implementation {
                name: "context-harness".to_string(),
                title: Some("Context Harness".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Context Harness — local-first context ingestion and retrieval for AI tools. \
                 Use the search tool to find relevant documents, get to retrieve a specific \
                 document by ID, and sources to list connector status. \
                 Agents are available as prompts — use list_prompts to discover them."
                    .to_string(),
            ),
        }
    }

    // ── Tools ────────────────────────────────────────────────────────────

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let mut tools: Vec<Tool> = self
            .tools
            .tools()
            .iter()
            .map(|t| Self::to_mcp_tool(t.as_ref()))
            .collect();
        for t in self.extra_tools.tools() {
            tools.push(Self::to_mcp_tool(t.as_ref()));
        }
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.find_tool(name).map(Self::to_mcp_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool = self.find_tool(&request.name).ok_or_else(|| {
            McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("no tool registered with name: {}", request.name),
                None,
            )
        })?;

        let params = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        let ctx = ToolContext::new(self.config.clone());
        match tool.execute(params, &ctx).await {
            Ok(result) => {
                let text = serde_json::to_string_pretty(&result).unwrap_or_default();
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    // ── Prompts (agents) ─────────────────────────────────────────────────

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        let mut prompts: Vec<Prompt> = self
            .agents
            .agents()
            .iter()
            .map(|a| Self::to_mcp_prompt(a.as_ref()))
            .collect();
        for a in self.extra_agents.agents() {
            prompts.push(Self::to_mcp_prompt(a.as_ref()));
        }
        std::future::ready(Ok(ListPromptsResult::with_all_items(prompts)))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let agent = self.find_agent(&request.name).ok_or_else(|| {
            McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("no agent registered with name: {}", request.name),
                None,
            )
        })?;

        let args = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        let ctx = ToolContext::new(self.config.clone());
        let resolved = agent.resolve(args, &ctx).await.map_err(|e| {
            McpError::new(
                ErrorCode::INTERNAL_ERROR,
                format!("agent '{}': {}", request.name, e),
                None,
            )
        })?;

        let mut messages: Vec<PromptMessage> = Vec::new();

        // System prompt as a user-role message (MCP prompts don't have a
        // system role, so we prepend it as user context).
        if !resolved.system.is_empty() {
            messages.push(PromptMessage::new_text(
                PromptMessageRole::User,
                &resolved.system,
            ));
        }

        for msg in &resolved.messages {
            let role = match msg.role.as_str() {
                "assistant" => PromptMessageRole::Assistant,
                _ => PromptMessageRole::User,
            };
            messages.push(PromptMessage::new_text(role, &msg.content));
        }

        Ok(GetPromptResult {
            description: Some(agent.description().to_string()),
            messages,
        })
    }
}
