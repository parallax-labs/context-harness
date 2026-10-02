//! Native, non-streaming Ollama chat adapter with a loopback-only transport.

use super::*;
use anyhow::{ensure, Context};
use reqwest::{redirect::Policy, Client, Url};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, net::IpAddr, time::Duration};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";
const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

pub struct OllamaProvider {
    model: String,
    endpoint: Url,
    client: Client,
}

impl OllamaProvider {
    pub fn from_definition(definition: &ModelDefinition) -> anyhow::Result<Self> {
        let (base_url, timeout_seconds) = effective_config(definition)?;
        let mut endpoint = Url::parse(&base_url)?;
        endpoint.set_path("/api/chat");
        Ok(Self {
            model: definition.model.clone(),
            endpoint,
            client: Client::builder()
                .timeout(Duration::from_secs(timeout_seconds))
                .connect_timeout(Duration::from_secs(10))
                .redirect(Policy::none())
                .no_proxy()
                .build()?,
        })
    }

    fn body(&self, request: &ModelRequest) -> ModelResult<Vec<u8>> {
        request.validate()?;
        let mut messages = Vec::new();
        let mut calls = BTreeMap::new();
        for message in &request.messages {
            match message {
                ModelMessage::System { content } => {
                    messages.push(json!({"role":"system", "content":content}));
                }
                ModelMessage::User { content } => {
                    messages.push(json!({"role":"user", "content":content}));
                }
                ModelMessage::Assistant {
                    text, tool_calls, ..
                } => {
                    let tool_calls: Vec<_> = tool_calls
                        .iter()
                        .map(|call| {
                            calls.insert(call.id.as_str(), wire_name(&call.name));
                            json!({"type":"function", "function":{
                                "name":wire_name(&call.name), "arguments":call.arguments
                            }})
                        })
                        .collect();
                    messages.push(json!({
                        "role":"assistant", "content":text, "tool_calls":tool_calls
                    }));
                }
                ModelMessage::Tool { call_id, content } => {
                    let name = calls
                        .get(call_id.as_str())
                        .ok_or_else(|| ModelError::new(ModelErrorKind::InvalidRequest))?;
                    messages.push(json!({
                        "role":"tool", "tool_name":name, "content":content
                    }));
                }
            }
        }

        let tools: Vec<_> = request
            .tools
            .iter()
            .map(|tool| {
                json!({"type":"function", "function":{
                    "name":wire_name(&tool.name),
                    "description":format!("{}: {}", tool.name, tool.description),
                    "parameters":tool.parameters
                }})
            })
            .collect();
        let mut body = Map::from_iter([
            ("model".into(), json!(self.model)),
            ("messages".into(), Value::Array(messages)),
            ("stream".into(), Value::Bool(false)),
            ("think".into(), Value::Bool(false)),
        ]);
        if !tools.is_empty() {
            body.insert("tools".into(), Value::Array(tools));
        }
        if let Some(output) = &request.output_schema {
            body.insert("format".into(), output.schema.clone());
        }
        if let Some(limit) = request.max_output_tokens {
            body.insert("options".into(), json!({"num_predict":limit}));
        }
        let bytes = serde_json::to_vec(&body)
            .map_err(|_| ModelError::new(ModelErrorKind::InvalidRequest))?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(ModelError::new(ModelErrorKind::InvalidRequest));
        }
        Ok(bytes)
    }

    fn decode(&self, body: Value, request: &ModelRequest) -> ModelResult<ModelResponse> {
        let invalid = || ModelError::new(ModelErrorKind::InvalidResponse);
        if body["done"].as_bool() != Some(true)
            || body["model"].as_str().is_none_or(str::is_empty)
            || body["message"]["role"].as_str() != Some("assistant")
        {
            return Err(invalid());
        }
        let text = body["message"]["content"]
            .as_str()
            .ok_or_else(invalid)?
            .to_owned();
        let names: BTreeMap<_, _> = request
            .tools
            .iter()
            .map(|tool| (wire_name(&tool.name), tool.name.clone()))
            .collect();
        let assistant_ordinal = request
            .messages
            .iter()
            .filter(|message| matches!(message, ModelMessage::Assistant { .. }))
            .count();
        let raw_calls = match body["message"].get("tool_calls") {
            None | Some(Value::Null) => &[][..],
            Some(value) => value.as_array().ok_or_else(invalid)?.as_slice(),
        };
        let mut tool_calls = Vec::with_capacity(raw_calls.len());
        for (index, call) in raw_calls.iter().enumerate() {
            let wire = call["function"]["name"].as_str().ok_or_else(invalid)?;
            let name = names.get(wire).ok_or_else(invalid)?.clone();
            let arguments = call["function"]["arguments"].clone();
            if !arguments.is_object() {
                return Err(invalid());
            }
            tool_calls.push(ToolCall {
                id: format!("ollama-call-{assistant_ordinal}-{index}"),
                name,
                arguments,
            });
        }
        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            match body.get("done_reason").and_then(Value::as_str) {
                None | Some("") | Some("stop") => FinishReason::Completed,
                Some("length") => FinishReason::Length,
                _ => return Err(invalid()),
            }
        };
        let usage = match (
            body.get("prompt_eval_count").and_then(Value::as_u64),
            body.get("eval_count").and_then(Value::as_u64),
        ) {
            (Some(input_tokens), Some(output_tokens)) => Some(Usage {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens
                    .checked_add(output_tokens)
                    .ok_or_else(invalid)?,
            }),
            _ => None,
        };
        let structured_output =
            if request.output_schema.is_some() && finish_reason == FinishReason::Completed {
                Some(serde_json::from_str(&text).map_err(|_| invalid())?)
            } else {
                None
            };
        let response = ModelResponse {
            text,
            tool_calls,
            usage,
            finish_reason,
            structured_output,
            continuation: None,
        };
        response.validate(request)?;
        Ok(response)
    }
}

pub(crate) fn validate_definition(definition: &ModelDefinition) -> anyhow::Result<()> {
    effective_config(definition).map(|_| ())
}

pub(crate) fn effective_config(definition: &ModelDefinition) -> anyhow::Result<(String, u64)> {
    definition.validate()?;
    ensure!(
        definition.provider == "ollama",
        "Ollama provider requires provider = 'ollama'"
    );
    ensure!(
        definition.api_key_env.is_none(),
        "api_key_env is not supported by the ollama provider"
    );
    let raw = definition.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);
    let url = Url::parse(raw).context("invalid Ollama base_url")?;
    ensure!(url.scheme() == "http", "Ollama base_url must use http");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Ollama base_url must not contain credentials"
    );
    ensure!(
        url.path() == "/" && url.query().is_none() && url.fragment().is_none(),
        "Ollama base_url must be an origin without a path, query, or fragment"
    );
    let host = url.host_str().context("Ollama base_url host is required")?;
    let address: IpAddr = host
        .parse()
        .context("Ollama base_url host must be a literal loopback address")?;
    ensure!(address.is_loopback(), "Ollama base_url must be loopback");
    ensure!(
        url.port_or_known_default().is_some_and(|port| port > 0),
        "Ollama base_url port must be nonzero"
    );
    let timeout = definition
        .timeout_seconds
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    ensure!(
        (1..=3600).contains(&timeout),
        "Ollama timeout_seconds must be in 1..=3600"
    );
    Ok((url.as_str().trim_end_matches('/').into(), timeout))
}

fn transport_error(error: reqwest::Error) -> ModelError {
    ModelError::new(if error.is_timeout() {
        ModelErrorKind::Timeout
    } else if error.is_connect() {
        ModelErrorKind::Unavailable
    } else {
        ModelErrorKind::Transport
    })
}

async fn read_limited(mut response: reqwest::Response) -> ModelResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
            return Err(ModelError::new(ModelErrorKind::InvalidResponse));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse> {
        let body = self.body(request)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(ModelError {
                kind: ModelErrorKind::ProviderFailure,
                http_status: Some(status.as_u16()),
            });
        }
        let bytes = read_limited(response).await?;
        if !status.is_success() {
            let error = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|body| body["error"].as_str().map(str::to_owned))
                .ok_or_else(|| ModelError::new(ModelErrorKind::InvalidResponse))?;
            let lower = error.to_ascii_lowercase();
            let unsupported = lower.contains("does not support tools")
                || (lower.contains("does not support")
                    && (lower.contains("format") || lower.contains("structured output")));
            let kind = match status.as_u16() {
                404 => ModelErrorKind::ModelUnavailable,
                429 => ModelErrorKind::RateLimited,
                500..=599 => ModelErrorKind::Unavailable,
                400 | 422 if unsupported => ModelErrorKind::UnsupportedCapability,
                400 | 422 => ModelErrorKind::InvalidRequest,
                _ => ModelErrorKind::ProviderFailure,
            };
            return Err(ModelError {
                kind,
                http_status: Some(status.as_u16()),
            });
        }
        let body = serde_json::from_slice(&bytes)
            .map_err(|_| ModelError::new(ModelErrorKind::InvalidResponse))?;
        self.decode(body, request)
    }
}

#[cfg(test)]
mod tests;
