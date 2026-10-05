//! Compatibility context projection. Later bounded-selection work can replace
//! this builder without changing the execution loop or checkpoint envelope.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ProjectionMetadata {
    pub builder: String,
    pub strategy: String,
    pub categories: Vec<String>,
    pub message_count: usize,
    pub tool_count: usize,
}

pub(super) struct ProjectedContext {
    pub request: ModelRequest,
    pub metadata: ProjectionMetadata,
}

pub(super) trait RunContextBuilder: Send + Sync {
    fn project(
        &self,
        system: &str,
        input: &str,
        tools: Vec<ModelTool>,
        restored: Option<ModelRequest>,
    ) -> Result<ProjectedContext>;

    fn describe(&self, request: &ModelRequest) -> ProjectionMetadata;
}

pub(super) struct CompatibilityContextBuilder;

impl RunContextBuilder for CompatibilityContextBuilder {
    fn project(
        &self,
        system: &str,
        input: &str,
        tools: Vec<ModelTool>,
        restored: Option<ModelRequest>,
    ) -> Result<ProjectedContext> {
        let request = restored.unwrap_or_else(|| ModelRequest {
            messages: vec![
                ModelMessage::System {
                    content: system.into(),
                },
                ModelMessage::User {
                    content: input.into(),
                },
            ],
            tools,
            ..Default::default()
        });
        request.validate()?;
        Ok(ProjectedContext {
            metadata: self.describe(&request),
            request,
        })
    }

    fn describe(&self, request: &ModelRequest) -> ProjectionMetadata {
        ProjectionMetadata {
            builder: "compatibility_v1".into(),
            strategy: "complete_transcript".into(),
            categories: vec![
                "system_prompt".into(),
                "objective".into(),
                "complete_transcript".into(),
                "tool_declarations".into(),
            ],
            message_count: request.messages.len(),
            tool_count: request.tools.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_builder_preserves_fresh_request_shape() {
        let projected = CompatibilityContextBuilder
            .project("system", "objective", Vec::new(), None)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&projected.request.messages).unwrap(),
            serde_json::to_value(vec![
                ModelMessage::System {
                    content: "system".into()
                },
                ModelMessage::User {
                    content: "objective".into()
                }
            ])
            .unwrap()
        );
        assert_eq!(projected.metadata.builder, "compatibility_v1");
        assert_eq!(projected.metadata.strategy, "complete_transcript");
    }

    #[test]
    fn compatibility_builder_does_not_rewrite_restored_requests() {
        let restored = ModelRequest {
            messages: vec![ModelMessage::User {
                content: "restored".into(),
            }],
            ..Default::default()
        };
        let projected = CompatibilityContextBuilder
            .project("ignored", "ignored", Vec::new(), Some(restored.clone()))
            .unwrap();
        assert_eq!(
            serde_json::to_value(projected.request).unwrap(),
            serde_json::to_value(restored).unwrap()
        );
        assert_eq!(projected.metadata.message_count, 1);
    }
}
