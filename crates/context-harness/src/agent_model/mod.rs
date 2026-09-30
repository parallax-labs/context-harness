//! Provider-neutral model calls, independent of the agent execution loop.
//! Recorded calls persist metadata only; conversation snapshots belong to the
//! runtime. No credentials or raw provider errors enter the event log.

pub mod fake;
pub mod openai;

use anyhow::{ensure, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::agent_resource::ModelDefinition;
use crate::agent_store::AgentRunStore;

pub type ModelResult<T> = Result<T, ModelError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Opaque adapter-owned continuation, serialized with the conversation. It may
/// contain sensitive context; never include it in metadata events or diagnostics.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderContinuation {
    provider: String,
    items: Vec<Value>,
}
impl std::fmt::Debug for ProviderContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderContinuation")
            .field("provider", &self.provider)
            .field("item_count", &self.items.len())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ModelMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
        continuation: Option<ProviderContinuation>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSchema {
    pub name: String,
    pub schema: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRequest {
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ModelTool>,
    pub output_schema: Option<OutputSchema>,
    pub max_output_tokens: Option<u32>,
}

impl ModelRequest {
    pub fn validate(&self) -> ModelResult<()> {
        let invalid = || ModelError::new(ModelErrorKind::InvalidRequest);
        if self.messages.is_empty() || self.max_output_tokens == Some(0) {
            return Err(invalid());
        }
        let mut names = HashSet::new();
        for tool in &self.tools {
            if tool.name.trim().is_empty()
                || !names.insert(&tool.name)
                || !tool.parameters.is_object()
            {
                return Err(invalid());
            }
        }
        if self
            .output_schema
            .as_ref()
            .is_some_and(|s| s.name.is_empty() || !s.schema.is_object())
        {
            return Err(invalid());
        }
        let mut pending = HashSet::new();
        let mut seen = HashSet::new();
        for message in &self.messages {
            match message {
                ModelMessage::Tool { call_id, .. } => {
                    if !pending.remove(call_id.as_str()) {
                        return Err(invalid());
                    }
                }
                ModelMessage::Assistant { tool_calls, .. } => {
                    if !pending.is_empty() {
                        return Err(invalid());
                    }
                    for call in tool_calls {
                        if call.id.is_empty()
                            || call.name.is_empty()
                            || !call.arguments.is_object()
                            || !seen.insert(call.id.as_str())
                        {
                            return Err(invalid());
                        }
                        pending.insert(call.id.as_str());
                    }
                }
                _ if !pending.is_empty() => return Err(invalid()),
                _ => {}
            }
        }
        if !pending.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Completed,
    ToolCalls,
    Length,
    Refusal,
    ContentFilter,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub finish_reason: FinishReason,
    pub structured_output: Option<Value>,
    pub continuation: Option<ProviderContinuation>,
}
impl ModelResponse {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: vec![],
            usage: None,
            finish_reason: FinishReason::Completed,
            structured_output: None,
            continuation: None,
        }
    }

    pub fn message(&self) -> ModelMessage {
        ModelMessage::Assistant {
            text: self.text.clone(),
            tool_calls: self.tool_calls.clone(),
            continuation: self.continuation.clone(),
        }
    }

    fn validate(&self, request: &ModelRequest) -> ModelResult<()> {
        let invalid = || ModelError::new(ModelErrorKind::InvalidResponse);
        if (self.finish_reason == FinishReason::ToolCalls) != !self.tool_calls.is_empty() {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        let previous_ids: HashSet<_> = request
            .messages
            .iter()
            .flat_map(|message| match message {
                ModelMessage::Assistant { tool_calls, .. } => tool_calls
                    .iter()
                    .map(|call| call.id.as_str())
                    .collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect();
        for call in &self.tool_calls {
            if call.id.is_empty()
                || !ids.insert(&call.id)
                || previous_ids.contains(call.id.as_str())
                || !call.arguments.is_object()
                || !request.tools.iter().any(|t| t.name == call.name)
            {
                return Err(invalid());
            }
        }
        if request.output_schema.is_some() && self.finish_reason == FinishReason::Completed {
            let parsed: Value = serde_json::from_str(&self.text).map_err(|_| invalid())?;
            if self.structured_output.as_ref() != Some(&parsed) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

/// Safe diagnostic categories: never attach a raw HTTP body, URL or credential.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelErrorKind {
    InvalidRequest,
    InvalidResponse,
    MissingCredentials,
    Authentication,
    RateLimited,
    Unavailable,
    Timeout,
    Transport,
    ProviderFailure,
    ScriptExhausted,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelError {
    pub kind: ModelErrorKind,
    pub http_status: Option<u16>,
}
impl ModelError {
    pub fn new(kind: ModelErrorKind) -> Self {
        Self {
            kind,
            http_status: None,
        }
    }
}
impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "model call failed: {:?}", self.kind)?;
        if let Some(status) = self.http_status {
            write!(f, " (HTTP {status})")?;
        }
        Ok(())
    }
}
impl std::error::Error for ModelError {}

/// One configured provider/model. The model name and credentials are bound at
/// construction, so requests cannot override their registry alias's model.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse>;
}

struct RegisteredModel {
    provider_name: String,
    model_name: String,
    provider: Arc<dyn ModelProvider>,
}
#[derive(Default)]
pub struct ModelRegistry {
    models: BTreeMap<String, RegisteredModel>,
}
impl ModelRegistry {
    /// Configured fake models return a clearly labelled synthetic final answer.
    /// Tests can instead register a FakeModel with a scripted sequence.
    pub fn from_config(models: &BTreeMap<String, ModelDefinition>) -> anyhow::Result<Self> {
        let mut registry = Self::default();
        for (alias, definition) in models {
            definition
                .validate()
                .with_context(|| format!("model '{alias}'"))?;
            let provider: Arc<dyn ModelProvider> = match definition.provider.as_str() {
                "openai" => Arc::new(openai::OpenAiProvider::new(
                    &definition.model,
                    definition
                        .api_key_env
                        .as_deref()
                        .unwrap_or("OPENAI_API_KEY"),
                )?),
                "fake" => Arc::new(fake::FakeModel::new([Ok(ModelResponse::text(
                    "Synthetic response from FakeModel; no model service was called.",
                ))])),
                _ => anyhow::bail!(
                    "unsupported model provider '{}' for '{alias}'",
                    definition.provider
                ),
            };
            registry.register(alias, &definition.provider, &definition.model, provider)?;
        }
        Ok(registry)
    }

    pub fn register(
        &mut self,
        alias: &str,
        provider_name: &str,
        model_name: &str,
        provider: Arc<dyn ModelProvider>,
    ) -> anyhow::Result<()> {
        ensure!(
            !alias.trim().is_empty()
                && !provider_name.trim().is_empty()
                && !model_name.trim().is_empty(),
            "model identity is required"
        );
        ensure!(
            !self.models.contains_key(alias),
            "duplicate model alias '{alias}'"
        );
        self.models.insert(
            alias.into(),
            RegisteredModel {
                provider_name: provider_name.into(),
                model_name: model_name.into(),
                provider,
            },
        );
        Ok(())
    }

    /// Stable, non-secret binding used to validate persisted runtime checkpoints.
    pub fn identity(&self, alias: &str) -> anyhow::Result<(&str, &str)> {
        let model = self.models.get(alias).context("unknown model alias")?;
        Ok((&model.provider_name, &model.model_name))
    }

    pub async fn generate(
        &self,
        alias: &str,
        request: &ModelRequest,
    ) -> anyhow::Result<ModelResponse> {
        let model = self
            .models
            .get(alias)
            .with_context(|| format!("unknown model alias '{alias}'"))?;
        request.validate()?;
        let response = model.provider.generate(request).await?;
        response.validate(request)?;
        Ok(response)
    }

    /// Persist lifecycle metadata around one model call. Run completion remains
    /// the caller's responsibility. Cancellation leaves an unmatched requested
    /// event; safe reconciliation belongs to the future resume layer.
    pub async fn generate_recorded(
        &self,
        store: &AgentRunStore,
        run_id: &str,
        alias: &str,
        request: &ModelRequest,
    ) -> anyhow::Result<ModelResponse> {
        let model = self
            .models
            .get(alias)
            .with_context(|| format!("unknown model alias '{alias}'"))?;
        request.validate()?;
        let run = store
            .get_run(run_id)
            .await?
            .context("run not found in workspace")?;
        ensure!(run.model == alias, "model alias does not match run binding");
        let call_id = Uuid::new_v4().to_string();
        store.append_event(run_id, "model.requested", &json!({
            "call_id": call_id, "model_alias": alias, "provider": model.provider_name,
            "model": model.model_name, "message_count": request.messages.len(),
            "tool_count": request.tools.len(), "structured_output": request.output_schema.is_some()
        })).await?;
        let result = model.provider.generate(request).await.and_then(|response| {
            response.validate(request)?;
            Ok(response)
        });
        match result {
            Ok(response) => {
                store
                    .append_event(
                        run_id,
                        "model.responded",
                        &json!({
                            "call_id": call_id, "finish_reason": response.finish_reason,
                            "usage": response.usage, "tool_call_count": response.tool_calls.len()
                        }),
                    )
                    .await?;
                Ok(response)
            }
            Err(error) => {
                store
                    .append_event(
                        run_id,
                        "model.failed",
                        &json!({"call_id": call_id, "error": error}),
                    )
                    .await?;
                Err(error.into())
            }
        }
    }
}
