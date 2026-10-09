//! Deterministic request projection. Compatibility remains the default; bounded
//! projection is enabled only by an explicit resource budget.
use super::*;
use crate::agent_store::WorkingStateInspection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SourceRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ProjectionMetadata {
    pub builder: String,
    pub strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub included_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_message_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub included_exchange_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted_exchange_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retained_source_ranges: Option<Vec<SourceRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_state_projection_version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_state_revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_state_event_cursor: Option<i64>,
}
pub(super) struct ProjectedContext {
    pub request: ModelRequest,
    pub metadata: ProjectionMetadata,
    pub retained_source: Option<ModelRequest>,
    pub retained_indexes: Option<Vec<u64>>,
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
            retained_source: None,
            retained_indexes: None,
        })
    }
    fn describe(&self, request: &ModelRequest) -> ProjectionMetadata {
        ProjectionMetadata {
            builder: "compatibility_v1".into(),
            strategy: "complete_transcript".into(),
            categories: Some(vec![
                "system_prompt".into(),
                "objective".into(),
                "complete_transcript".into(),
                "tool_declarations".into(),
            ]),
            message_count: Some(request.messages.len()),
            tool_count: Some(request.tools.len()),
            max_context_bytes: None,
            request_bytes: None,
            source_message_count: None,
            included_message_count: None,
            omitted_message_count: None,
            included_exchange_count: None,
            omitted_exchange_count: None,
            retained_source_ranges: None,
            working_state_projection_version: None,
            working_state_revision: None,
            working_state_event_cursor: None,
        }
    }
}

#[derive(Serialize)]
struct WorkingStateCapsule<'a> {
    r#type: &'static str,
    projection_version: i64,
    revision: i64,
    event_cursor: i64,
    snapshot: &'a crate::agent_store::WorkingStateSnapshot,
}

pub(super) fn bounded_project(
    source: &ModelRequest,
    max_bytes: u64,
    working: &WorkingStateInspection,
    logical_indexes: &[u64],
    source_message_count: u64,
    source_exchange_count: u64,
) -> Result<ProjectedContext> {
    ensure!(
        source.messages.len() >= 2 && logical_indexes.len() == source.messages.len(),
        "invalid bounded projection source"
    );
    let snapshot = working
        .snapshot
        .as_ref()
        .context("working state snapshot is unavailable")?;
    let projection_version = working
        .projection_version
        .context("working state projection version is unavailable")?;
    let revision = working
        .revision
        .context("working state revision is unavailable")?;
    let event_cursor = working
        .event_cursor
        .context("working state event cursor is unavailable")?;
    let capsule = serde_json::to_string(&WorkingStateCapsule {
        r#type: "working_state",
        projection_version,
        revision,
        event_cursor,
        snapshot,
    })?;
    let mut exchanges = Vec::new();
    let mut cursor = 2;
    while cursor < source.messages.len() {
        let start = cursor;
        ensure!(
            matches!(source.messages[cursor], ModelMessage::Assistant { .. }),
            "bounded source exchange must start with an assistant message"
        );
        cursor += 1;
        while cursor < source.messages.len()
            && matches!(source.messages[cursor], ModelMessage::Tool { .. })
        {
            cursor += 1;
        }
        exchanges.push((start, cursor));
    }
    let mut first = exchanges.len();
    if !exchanges.is_empty() {
        first -= 1;
        loop {
            let candidate = build_request(source, &capsule, &exchanges[first..]);
            if serde_json::to_vec(&candidate)?.len() as u64 > max_bytes {
                if first + 1 == exchanges.len() {
                    anyhow::bail!("context_bytes");
                }
                first += 1;
                break;
            }
            if first == 0 {
                break;
            }
            first -= 1;
        }
    }
    let request = build_request(source, &capsule, &exchanges[first..]);
    let request_bytes = serde_json::to_vec(&request)?.len() as u64;
    ensure!(request_bytes <= max_bytes, "context_bytes");
    let retained: Vec<usize> = [0, 1]
        .into_iter()
        .chain(exchanges[first..].iter().flat_map(|(s, e)| *s..*e))
        .collect();
    let included = retained.len() as u64;
    let mut retained_source = source.clone();
    retained_source.messages = retained
        .iter()
        .map(|position| source.messages[*position].clone())
        .collect();
    let retained_indexes = retained
        .iter()
        .map(|position| logical_indexes[*position])
        .collect();
    Ok(ProjectedContext {
        request,
        retained_source: Some(retained_source),
        retained_indexes: Some(retained_indexes),
        metadata: ProjectionMetadata {
            builder: "bounded_v1".into(),
            strategy: "bounded_suffix_v1".into(),
            categories: None,
            message_count: None,
            tool_count: None,
            max_context_bytes: Some(max_bytes),
            request_bytes: Some(request_bytes),
            source_message_count: Some(source_message_count),
            included_message_count: Some(included),
            omitted_message_count: Some(source_message_count.saturating_sub(included)),
            included_exchange_count: Some((exchanges.len() - first) as u64),
            omitted_exchange_count: Some(
                source_exchange_count.saturating_sub((exchanges.len() - first) as u64),
            ),
            retained_source_ranges: Some(coalesce(retained.iter().map(|p| logical_indexes[*p]))),
            working_state_projection_version: Some(projection_version),
            working_state_revision: Some(revision),
            working_state_event_cursor: Some(event_cursor),
        },
    })
}
pub(super) fn exchange_count(request: &ModelRequest) -> u64 {
    request
        .messages
        .iter()
        .filter(|message| matches!(message, ModelMessage::Assistant { .. }))
        .count() as u64
}
fn build_request(
    source: &ModelRequest,
    capsule: &str,
    exchanges: &[(usize, usize)],
) -> ModelRequest {
    let mut messages = vec![
        source.messages[0].clone(),
        ModelMessage::System {
            content: capsule.into(),
        },
        source.messages[1].clone(),
    ];
    for (start, end) in exchanges {
        messages.extend_from_slice(&source.messages[*start..*end]);
    }
    ModelRequest {
        messages,
        tools: source.tools.clone(),
        ..source.clone()
    }
}
fn coalesce(indexes: impl IntoIterator<Item = u64>) -> Vec<SourceRange> {
    let mut ranges: Vec<SourceRange> = Vec::new();
    for index in indexes {
        if let Some(last) = ranges.last_mut().filter(|r| r.end + 1 == index) {
            last.end = index;
        } else {
            ranges.push(SourceRange {
                start: index,
                end: index,
            });
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_store::{
        RunLifecycle, RunUsage, WorkingStateArtifacts, WorkingStateEventReference,
        WorkingStateObjective, WorkingStateRun, WorkingStateSnapshot, WorkingStateStatus,
    };
    fn working() -> WorkingStateInspection {
        WorkingStateInspection {
            status: WorkingStateStatus::Current,
            projection_version: Some(1),
            revision: Some(7),
            event_cursor: Some(9),
            updated_at: Some(10),
            snapshot: Some(WorkingStateSnapshot {
                objective: WorkingStateObjective {
                    kind: "agent_run".into(),
                    run_id: "run".into(),
                },
                run: WorkingStateRun {
                    lifecycle: RunLifecycle::Active,
                    outcome: None,
                    reason_code: None,
                },
                usage: RunUsage::default(),
                latest_event: WorkingStateEventReference {
                    sequence: 9,
                    event_type: "tool.completed".into(),
                },
                artifacts: WorkingStateArtifacts {
                    items: Vec::new(),
                    total: 0,
                    omitted: 0,
                },
            }),
        }
    }
    #[test]
    fn compatibility_builder_preserves_fresh_request_shape() {
        let p = CompatibilityContextBuilder
            .project("system", "objective", Vec::new(), None)
            .unwrap();
        assert_eq!(p.request.messages.len(), 2);
        assert_eq!(p.metadata.builder, "compatibility_v1");
    }
    #[test]
    fn compatibility_builder_does_not_rewrite_restored_requests() {
        let r = ModelRequest {
            messages: vec![ModelMessage::User {
                content: "restored".into(),
            }],
            ..Default::default()
        };
        let p = CompatibilityContextBuilder
            .project("ignored", "ignored", Vec::new(), Some(r.clone()))
            .unwrap();
        assert_eq!(
            serde_json::to_value(p.request).unwrap(),
            serde_json::to_value(r).unwrap()
        );
        assert_eq!(p.metadata.message_count, Some(1));
    }
    #[test]
    fn bounded_builder_keeps_prefix_and_newest_whole_exchange() {
        let source = ModelRequest {
            messages: vec![
                ModelMessage::System {
                    content: "system".into(),
                },
                ModelMessage::User {
                    content: "objective".into(),
                },
                ModelMessage::Assistant {
                    text: "x".repeat(2_000),
                    tool_calls: Vec::new(),
                    continuation: None,
                },
                ModelMessage::Assistant {
                    text: "newest".into(),
                    tool_calls: Vec::new(),
                    continuation: None,
                },
            ],
            ..Default::default()
        };
        let projected = bounded_project(&source, 1_000, &working(), &[0, 1, 2, 3], 4, 2).unwrap();
        assert_eq!(projected.metadata.omitted_exchange_count, Some(1));
        assert_eq!(
            projected.metadata.retained_source_ranges,
            Some(vec![
                SourceRange { start: 0, end: 1 },
                SourceRange { start: 3, end: 3 }
            ])
        );
        assert_eq!(
            serde_json::to_vec(&projected.request).unwrap().len() as u64,
            projected.metadata.request_bytes.unwrap()
        );
        assert!(
            matches!(&projected.request.messages[1],ModelMessage::System{content} if content.contains("\"projection_version\":1") && !content.contains("updated_at"))
        );
    }
}
