//! OpenAI Responses adapter: stateless, non-streaming, caller-owned history.
//! See SPEC-0016 for supported items and intentionally deferred streaming.
use super::*;
use reqwest::{redirect::Policy, Client};
use std::time::Duration;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub struct OpenAiProvider {
    model: String,
    api_key_env: String,
    endpoint: String,
    client: Client,
    #[cfg(test)]
    test_key: Option<String>,
}
impl OpenAiProvider {
    pub fn new(model: &str, api_key_env: &str) -> anyhow::Result<Self> {
        ModelDefinition {
            provider: "openai".into(),
            model: model.into(),
            api_key_env: Some(api_key_env.into()),
            ..Default::default()
        }
        .validate()?;
        Ok(Self {
            model: model.into(),
            api_key_env: api_key_env.into(),
            endpoint: "https://api.openai.com/v1/responses".into(),
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .connect_timeout(Duration::from_secs(10))
                .redirect(Policy::none())
                .build()?,
            #[cfg(test)]
            test_key: None,
        })
    }

    fn credential(&self) -> ModelResult<String> {
        #[cfg(test)]
        if let Some(key) = &self.test_key {
            return Ok(key.clone());
        }
        std::env::var(&self.api_key_env)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ModelError::new(ModelErrorKind::MissingCredentials))
    }

    fn body(&self, request: &ModelRequest) -> ModelResult<Value> {
        request.validate()?;
        let mut input = Vec::new();
        for message in &request.messages {
            match message {
                ModelMessage::System { content } => {
                    input.push(json!({"role": "system", "content": content}))
                }
                ModelMessage::User { content } => {
                    input.push(json!({"role": "user", "content": content}))
                }
                ModelMessage::Tool { call_id, content } => input.push(
                    json!({"type": "function_call_output", "call_id": call_id, "output": content}),
                ),
                ModelMessage::Assistant {
                    text,
                    tool_calls,
                    continuation,
                } => {
                    if let Some(state) = continuation {
                        if state.provider != "openai" {
                            return Err(ModelError::new(ModelErrorKind::InvalidRequest));
                        }
                        let names = tool_calls
                            .iter()
                            .map(|call| (wire_name(&call.name), call.name.clone()))
                            .collect();
                        let (saved_text, saved_calls, _) = decode_items(&state.items, &names, true)
                            .map_err(|_| ModelError::new(ModelErrorKind::InvalidRequest))?;
                        if saved_text != *text || saved_calls != *tool_calls {
                            return Err(ModelError::new(ModelErrorKind::InvalidRequest));
                        }
                        input.extend(state.items.clone());
                    } else {
                        if !text.is_empty() {
                            input.push(json!({"role": "assistant", "content": text}));
                        }
                        for call in tool_calls {
                            input.push(json!({"type": "function_call", "call_id": call.id,
                                "name": wire_name(&call.name), "arguments": call.arguments.to_string()}));
                        }
                    }
                }
            }
        }
        // Registry names may contain dots; OpenAI function names cannot. Stable
        // 64-character hashes preserve distinct names without lossy substitution.
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function", "name": wire_name(&tool.name),
                    "description": format!("{}: {}", tool.name, tool.description),
                    "parameters": tool.parameters, "strict": false
                })
            })
            .collect();
        let mut body = json!({"model": self.model, "input": input, "tools": tools,
            "store": false, "stream": false, "include": ["reasoning.encrypted_content"]});
        if let Some(limit) = request.max_output_tokens {
            body["max_output_tokens"] = json!(limit);
        }
        if let Some(output) = &request.output_schema {
            body["text"] = json!({"format": {"type": "json_schema", "name": output.name, "schema": output.schema, "strict": true}});
        }
        Ok(body)
    }

    fn decode(&self, body: Value, request: &ModelRequest) -> ModelResult<ModelResponse> {
        let invalid = || ModelError::new(ModelErrorKind::InvalidResponse);
        let status = body["status"].as_str().ok_or_else(invalid)?;
        if status == "failed" {
            return Err(ModelError::new(ModelErrorKind::ProviderFailure));
        }
        if status != "completed" && status != "incomplete" {
            return Err(invalid());
        }
        let items = body["output"].as_array().ok_or_else(invalid)?;
        let names = request
            .tools
            .iter()
            .map(|tool| (wire_name(&tool.name), tool.name.clone()))
            .collect();
        let (text, mut tool_calls, refused) = decode_items(items, &names, status == "completed")?;
        let finish_reason = if status == "incomplete" {
            // Partial calls are not executable, even if their arguments parse.
            tool_calls.clear();
            match body["incomplete_details"]["reason"].as_str() {
                Some("max_output_tokens") => FinishReason::Length,
                Some("content_filter") => FinishReason::ContentFilter,
                _ => return Err(invalid()),
            }
        } else if refused {
            tool_calls.clear();
            FinishReason::Refusal
        } else if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            if text.is_empty() {
                return Err(invalid());
            }
            FinishReason::Completed
        };
        let usage = match body.get("usage").filter(|v| !v.is_null()) {
            Some(value) => Some(serde_json::from_value(value.clone()).map_err(|_| invalid())?),
            None => None,
        };
        let structured_output =
            if request.output_schema.is_some() && finish_reason == FinishReason::Completed {
                Some(serde_json::from_str(&text).map_err(|_| invalid())?)
            } else {
                None
            };
        // Incomplete/refused responses cannot be resumed as successful turns.
        let continuation = if matches!(
            finish_reason,
            FinishReason::Completed | FinishReason::ToolCalls
        ) {
            Some(ProviderContinuation {
                provider: "openai".into(),
                items: items.clone(),
            })
        } else {
            None
        };
        let response = ModelResponse {
            text,
            tool_calls,
            usage,
            finish_reason,
            structured_output,
            continuation,
        };
        response.validate(request)?;
        Ok(response)
    }
}

fn decode_items(
    items: &[Value],
    names: &BTreeMap<String, String>,
    complete: bool,
) -> ModelResult<(String, Vec<ToolCall>, bool)> {
    let invalid = || ModelError::new(ModelErrorKind::InvalidResponse);
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut refused = false;
    for item in items {
        match item["type"].as_str() {
            Some("reasoning") => {} // Preserve the full item for the next request.
            Some("message") => {
                if item["role"].as_str() != Some("assistant") {
                    return Err(invalid());
                }
                for content in item["content"].as_array().ok_or_else(invalid)? {
                    match content["type"].as_str() {
                        Some("output_text") => {
                            text.push_str(content["text"].as_str().ok_or_else(invalid)?)
                        }
                        Some("refusal") => {
                            refused = true;
                            text.push_str(content["refusal"].as_str().ok_or_else(invalid)?);
                        }
                        _ => return Err(invalid()),
                    }
                }
            }
            Some("function_call") if !complete => {} // Truncated arguments are not executable.
            Some("function_call") => {
                let name = names
                    .get(item["name"].as_str().ok_or_else(invalid)?)
                    .ok_or_else(invalid)?
                    .clone();
                calls.push(ToolCall {
                    id: item["call_id"].as_str().ok_or_else(invalid)?.into(),
                    name,
                    arguments: serde_json::from_str(
                        item["arguments"].as_str().ok_or_else(invalid)?,
                    )
                    .map_err(|_| invalid())?,
                });
            }
            _ => return Err(invalid()), // Never silently discard unsupported output.
        }
    }
    Ok((text, calls, refused))
}

fn transport_error(error: reqwest::Error) -> ModelError {
    ModelError::new(if error.is_timeout() {
        ModelErrorKind::Timeout
    } else {
        ModelErrorKind::Transport
    })
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    async fn generate(&self, request: &ModelRequest) -> ModelResult<ModelResponse> {
        let body = self.body(request)?;
        let key = self.credential()?;
        let mut response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            // Error bodies can echo prompts or credentials; do not read/log them.
            let kind = match status.as_u16() {
                401 | 403 => ModelErrorKind::Authentication,
                429 => ModelErrorKind::RateLimited,
                500..=599 => ModelErrorKind::Unavailable,
                400 | 404 | 422 => ModelErrorKind::InvalidRequest,
                _ => ModelErrorKind::ProviderFailure,
            };
            return Err(ModelError {
                kind,
                http_status: Some(status.as_u16()),
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(ModelError::new(ModelErrorKind::InvalidResponse));
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice(&bytes)
            .map_err(|_| ModelError::new(ModelErrorKind::InvalidResponse))?;
        self.decode(body, request)
    }
}

#[cfg(test)]
mod tests;
