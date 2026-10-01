//! Bounded stdio MCP sessions. Authorization must precede `connect`: spawning
//! a server is process execution even when its advertised tools claim read-only.
use crate::{
    agent_resource::Capability,
    config::McpServerConfig,
    tool_binding::{self, LoadedToolResource, ResolvedToolBinding},
    traits::{Tool, ToolContext, ToolRegistry},
};
use anyhow::{anyhow, ensure, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use rmcp::{
    model::{
        CallToolRequestParams, ClientInfo, ClientResult, PaginatedRequestParams,
        ServerNotification, ServerRequest, TaskSupport,
    },
    service::{NotificationContext, RequestContext, RunningService},
    transport::Transport,
    ErrorData, Peer, RoleClient, Service, ServiceExt,
};
use serde_json::Value;
use std::{collections::HashSet, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

const MAX_FRAME: usize = 1024 * 1024;
const MAX_ARGUMENTS: usize = 64 * 1024;
type Incoming = rmcp::service::RxJsonRpcMessage<RoleClient>;
type Outgoing = rmcp::service::TxJsonRpcMessage<RoleClient>;
struct BoundedTransport {
    reader: FramedRead<ChildStdout, LinesCodec>,
    writer: Arc<Mutex<Option<FramedWrite<ChildStdin, LinesCodec>>>>,
    ended: bool,
}
impl Transport<RoleClient> for BoundedTransport {
    type Error = std::io::Error;
    fn send(
        &mut self,
        message: Outgoing,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send + 'static {
        let writer = self.writer.clone();
        async move {
            let bytes = serde_json::to_vec(&message)
                .map_err(|_| std::io::Error::other("MCP request encoding failed"))?;
            if bytes.len() > MAX_FRAME {
                return Err(std::io::Error::other("MCP request exceeds size limit"));
            }
            let mut writer = writer.lock().await;
            writer
                .as_mut()
                .ok_or_else(|| std::io::Error::other("MCP transport closed"))?
                .send(
                    String::from_utf8(bytes)
                        .map_err(|_| std::io::Error::other("MCP request encoding failed"))?,
                )
                .await
                .map_err(|_| std::io::Error::other("MCP write failed"))
        }
    }
    async fn receive(&mut self) -> Option<Incoming> {
        if self.ended {
            return None;
        }
        match self.reader.next().await {
            Some(Ok(line)) => match serde_json::from_str(&line) {
                Ok(message) => Some(message),
                Err(_) => {
                    self.ended = true;
                    None
                }
            },
            _ => {
                self.ended = true;
                None
            }
        }
    }
    async fn close(&mut self) -> std::io::Result<()> {
        self.ended = true;
        self.writer.lock().await.take();
        Ok(())
    }
}
/// No sampling, roots, elicitation, or server-initiated execution is supported.
struct EmptyClient;
impl Service<RoleClient> for EmptyClient {
    async fn handle_request(
        &self,
        _: ServerRequest,
        _: RequestContext<RoleClient>,
    ) -> Result<ClientResult, ErrorData> {
        Err(ErrorData::invalid_request(
            "client requests are unsupported",
            None,
        ))
    }
    async fn handle_notification(
        &self,
        _: ServerNotification,
        _: NotificationContext<RoleClient>,
    ) -> Result<(), ErrorData> {
        Ok(())
    }
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

pub(super) struct Session {
    // Child stays with this guard, never in a detached SDK service task.
    child: Child,
    service: RunningService<RoleClient, EmptyClient>,
    tools: Vec<RemoteTool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.service.cancellation_token().cancel();
        let _ = self.child.start_kill();
    }
}
impl Session {
    pub(super) fn register(&self, registry: &mut ToolRegistry) -> Result<()> {
        ensure!(
            self.tools
                .iter()
                .all(|tool| registry.find(&tool.name).is_none()),
            "duplicate MCP tool namespace"
        );
        for tool in &self.tools {
            registry.register(Box::new(tool.clone()));
        }
        Ok(())
    }
    pub(super) async fn close(&mut self) {
        let _ = self.child.start_kill();
        let _ = self
            .service
            .close_with_timeout(Duration::from_secs(1))
            .await;
        let _ = tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await;
    }

    pub(super) fn register_alias(
        &self,
        resource: &LoadedToolResource,
        server: &McpServerConfig,
        registry: &mut ToolRegistry,
    ) -> Result<()> {
        let (_, remote_name) =
            tool_binding::mcp_reference(&resource.definition.tool.implementation)
                .ok_or_else(|| anyhow!("invalid MCP alias implementation"))?;
        let remote = self
            .tools
            .iter()
            .find(|tool| tool.remote == remote_name)
            .ok_or_else(|| anyhow!("MCP alias remote tool was not discovered"))?;
        let binding = tool_binding::resolve_mcp_binding(
            &resource.definition,
            &remote.schema,
            &remote.description,
            &serde_json::to_value(server)?,
        )?;
        ensure!(
            registry.find(&binding.name).is_none(),
            "duplicate MCP alias"
        );
        registry.register(Box::new(RemoteAlias {
            remote: remote.clone(),
            binding,
        }));
        Ok(())
    }
}

pub(super) async fn connect(name: &str, config: &McpServerConfig, root: &Path) -> Result<Session> {
    config.validate(name)?;
    let timeout = Duration::from_secs(config.timeout_seconds);
    let mut child = Command::new(&config.command)
        .args(&config.args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| anyhow!("MCP process could not start"))?;
    let reader = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("MCP stdout unavailable"))?;
    let writer = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("MCP stdin unavailable"))?;
    let transport = BoundedTransport {
        reader: FramedRead::new(reader, LinesCodec::new_with_max_length(MAX_FRAME)),
        writer: Arc::new(Mutex::new(Some(FramedWrite::new(
            writer,
            LinesCodec::new_with_max_length(MAX_FRAME),
        )))),
        ended: false,
    };
    let service = tokio::time::timeout(timeout, EmptyClient.serve(transport))
        .await
        .map_err(|_| anyhow!("MCP initialization timed out"))?
        .map_err(|_| anyhow!("MCP initialization failed"))?;
    let mut session = Session {
        child,
        service,
        tools: vec![],
    };
    let mut cursor = None;
    let mut cursors = HashSet::new();
    let mut names = HashSet::new();
    for page in 0..16 {
        let result = tokio::time::timeout(
            timeout,
            session
                .service
                .list_tools(Some(PaginatedRequestParams { meta: None, cursor })),
        )
        .await
        .map_err(|_| anyhow!("MCP discovery timed out"))?
        .map_err(|_| anyhow!("MCP discovery failed"))?;
        ensure!(
            session.tools.len() + result.tools.len() <= 128,
            "MCP discovery exceeds tool limit"
        );
        for tool in result.tools {
            ensure!(
                !tool.name.is_empty()
                    && tool.name.len() <= 128
                    && tool
                        .name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')),
                "invalid MCP tool name"
            );
            ensure!(
                names.insert(tool.name.to_string()),
                "duplicate MCP tool name"
            );
            ensure!(
                tool.input_schema.get("type").and_then(Value::as_str) == Some("object"),
                "MCP tool schema must declare object type"
            );
            ensure!(
                tool.execution
                    .as_ref()
                    .and_then(|execution| execution.task_support)
                    != Some(TaskSupport::Required),
                "MCP task-only tools are unsupported"
            );
            session.tools.push(RemoteTool {
                name: format!("mcp.{name}.{}", tool.name),
                remote: tool.name.into_owned(),
                description: tool
                    .description
                    .map(|description| description.into_owned())
                    .unwrap_or_default(),
                schema: Value::Object((*tool.input_schema).clone()),
                peer: session.service.peer().clone(),
                timeout,
            });
        }
        cursor = result.next_cursor;
        if let Some(next) = &cursor {
            ensure!(
                page < 15 && next.len() <= 4096 && cursors.insert(next.clone()),
                "MCP discovery pagination limit"
            );
        } else {
            return Ok(session);
        }
    }
    unreachable!()
}

pub(super) fn validate_arguments(arguments: &Value) -> Result<()> {
    ensure!(arguments.is_object(), "MCP arguments must be an object");
    ensure!(
        serde_json::to_vec(arguments)?.len() <= MAX_ARGUMENTS,
        "MCP arguments exceed size limit"
    );
    Ok(())
}
#[derive(Clone)]
struct RemoteTool {
    name: String,
    remote: String,
    description: String,
    schema: Value,
    peer: Peer<RoleClient>,
    timeout: Duration,
}

#[derive(Clone)]
struct RemoteAlias {
    remote: RemoteTool,
    binding: ResolvedToolBinding,
}

impl RemoteAlias {
    fn effective_arguments(&self, arguments: Value) -> Result<Value> {
        tool_binding::validate_binding_arguments(&self.binding.public_schema, &arguments)?;
        let mut arguments = arguments
            .as_object()
            .ok_or_else(|| anyhow!("MCP arguments must be an object"))?
            .clone();
        for (name, value) in self
            .binding
            .fixed
            .as_object()
            .ok_or_else(|| anyhow!("fixed arguments must be an object"))?
        {
            ensure!(
                !arguments.contains_key(name),
                "fixed argument cannot be overridden"
            );
            arguments.insert(name.clone(), value.clone());
        }
        let arguments = Value::Object(arguments);
        tool_binding::validate_binding_arguments(&self.remote.schema, &arguments)?;
        Ok(arguments)
    }
}

#[async_trait]
impl Tool for RemoteAlias {
    fn name(&self) -> &str {
        &self.binding.name
    }
    fn description(&self) -> &str {
        &self.binding.description
    }
    fn parameters_schema(&self) -> Value {
        self.binding.public_schema.clone()
    }
    fn capabilities(&self) -> Option<Vec<Capability>> {
        Some(self.binding.capabilities.clone())
    }
    fn binding_metadata(&self) -> Option<Value> {
        let mut metadata = tool_binding::binding_metadata(&self.binding);
        metadata["remote_metadata"] = serde_json::json!(self.binding.remote_metadata);
        Some(metadata)
    }
    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        tool_binding::validate_binding_arguments(&self.binding.public_schema, arguments)
    }
    fn approval_arguments(&self, arguments: &Value) -> Result<Value> {
        self.effective_arguments(arguments.clone())
    }
    async fn execute(&self, arguments: Value, context: &ToolContext) -> Result<Value> {
        let result = self
            .remote
            .execute(self.effective_arguments(arguments)?, context)
            .await?;
        let limit = self
            .binding
            .restrictions
            .max_output_bytes
            .unwrap_or(MAX_FRAME as u64);
        ensure!(
            serde_json::to_vec(&result)?.len() as u64 <= limit,
            "MCP alias result exceeds output limit"
        );
        Ok(result)
    }
}
#[async_trait]
impl Tool for RemoteTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }
    fn capabilities(&self) -> Option<Vec<Capability>> {
        Some(vec![
            Capability::ProcessExecute,
            Capability::ExternalSideEffect,
        ])
    }
    fn validate_arguments(&self, arguments: &Value) -> Result<()> {
        validate_arguments(arguments)
    }
    async fn execute(&self, arguments: Value, _: &ToolContext) -> Result<Value> {
        validate_arguments(&arguments)?;
        let result = tokio::time::timeout(
            self.timeout,
            self.peer.call_tool(CallToolRequestParams {
                meta: None,
                name: self.remote.clone().into(),
                arguments: arguments.as_object().cloned(),
                task: None,
            }),
        )
        .await
        .map_err(|_| anyhow!("MCP tool call timed out"))?
        .map_err(|_| anyhow!("MCP tool call failed"))?;
        ensure!(result.is_error != Some(true), "MCP tool returned an error");
        let bytes =
            serde_json::to_vec(&result).map_err(|_| anyhow!("MCP result encoding failed"))?;
        ensure!(bytes.len() <= MAX_FRAME, "MCP result exceeds size limit");
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("MCP result encoding failed"))
    }
}
