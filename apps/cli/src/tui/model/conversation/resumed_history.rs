use crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause;
use sdk::{
    LocalResumeContentBlock as ContentBlock, LocalResumeMessage as Message,
    LocalResumeMessageSource as MessageSource, LocalResumeRole as Role,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct ResumedHistoryStep {
    pub(crate) run_id: String,
    pub(crate) step_id: String,
    pub(crate) message_segments: Vec<Arc<[Message]>>,
    pub(crate) finalize_cause: Option<TuiResumedStepFinalizeCause>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResumedHistoryItemKind {
    UserMessage {
        message_index: usize,
    },
    AssistantText {
        message_index: usize,
        block_index: usize,
    },
    Thinking {
        message_index: usize,
        block_index: usize,
    },
    ToolCall {
        message_index: usize,
        block_index: usize,
    },
    ToolResult {
        message_index: usize,
        block_index: usize,
    },
    TerminalNotice,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResumedHistoryItem {
    pub(crate) id: String,
    pub(crate) estimated_lines: usize,
    pub(crate) step_index: usize,
    pub(crate) kind: ResumedHistoryItemKind,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ResumedHistoryBacking {
    steps: Vec<ResumedHistoryStep>,
    items: Vec<ResumedHistoryItem>,
}

impl PartialEq for ResumedHistoryBacking {
    fn eq(&self, other: &Self) -> bool {
        self.steps.len() == other.steps.len()
            && self.items == other.items
            && self.steps.iter().zip(&other.steps).all(|(left, right)| {
                left.run_id == right.run_id
                    && left.step_id == right.step_id
                    && left.finalize_cause == right.finalize_cause
                    && left.duration_ms == right.duration_ms
                    && serde_json::to_value(left.messages().collect::<Vec<_>>()).ok()
                        == serde_json::to_value(right.messages().collect::<Vec<_>>()).ok()
            })
    }
}

impl ResumedHistoryBacking {
    pub(crate) fn from_sdk(backing: sdk::LocalSessionResumeBacking) -> Self {
        let steps = backing
            .steps
            .into_iter()
            .map(|step| ResumedHistoryStep {
                run_id: step.run_id,
                step_id: step.step_id,
                message_segments: step.message_segments,
                finalize_cause: step.finalize_cause.map(|cause| match cause {
                    sdk::ResumedStepFinalizeCause::Completed => {
                        TuiResumedStepFinalizeCause::Completed
                    }
                    sdk::ResumedStepFinalizeCause::UserCancelledStep => {
                        TuiResumedStepFinalizeCause::UserCancelledStep
                    }
                    sdk::ResumedStepFinalizeCause::RunTerminated => {
                        TuiResumedStepFinalizeCause::RunTerminated
                    }
                }),
                duration_ms: step.duration_ms,
            })
            .collect::<Vec<_>>();
        let items = build_items(&steps);
        Self { steps, items }
    }

    pub(crate) fn steps(&self) -> &[ResumedHistoryStep] {
        &self.steps
    }

    pub(crate) fn items(&self) -> &[ResumedHistoryItem] {
        &self.items
    }

    pub(crate) fn item(&self, id: &str) -> Option<&ResumedHistoryItem> {
        self.items.iter().find(|item| item.id == id)
    }

    pub(crate) fn message_count(&self) -> usize {
        self.steps
            .iter()
            .flat_map(ResumedHistoryStep::messages)
            .count()
    }

    pub(crate) fn user_input_history(&self) -> Vec<String> {
        self.steps
            .iter()
            .flat_map(ResumedHistoryStep::messages)
            .filter(|message| {
                message.role == Role::User
                    && message
                        .metadata
                        .as_ref()
                        .is_none_or(|metadata| metadata.source == MessageSource::User)
                    && !message.has_tool_results()
            })
            .map(Message::text_content)
            .filter(|text| !text.trim().is_empty())
            .collect()
    }
}

fn build_items(steps: &[ResumedHistoryStep]) -> Vec<ResumedHistoryItem> {
    let mut items = Vec::new();
    for (step_index, step) in steps.iter().enumerate() {
        for (message_index, message) in step.messages().enumerate() {
            match message.role {
                Role::User if !message.has_tool_results() => {
                    let text = message.text_content();
                    items.push(ResumedHistoryItem {
                        id: format!("history-{step_index}-message-{message_index}"),
                        estimated_lines: text.lines().count().max(1).saturating_add(2),
                        step_index,
                        kind: ResumedHistoryItemKind::UserMessage { message_index },
                    });
                }
                _ => {
                    for (block_index, block) in message.content.iter().enumerate() {
                        let (kind, estimated_lines) = match block {
                            ContentBlock::Text { text } => (
                                ResumedHistoryItemKind::AssistantText {
                                    message_index,
                                    block_index,
                                },
                                text.lines().count().max(1).saturating_add(1),
                            ),
                            ContentBlock::Thinking { thinking, .. } => (
                                ResumedHistoryItemKind::Thinking {
                                    message_index,
                                    block_index,
                                },
                                thinking.lines().count().max(1).saturating_add(1),
                            ),
                            ContentBlock::ToolUse { .. } => (
                                ResumedHistoryItemKind::ToolCall {
                                    message_index,
                                    block_index,
                                },
                                10,
                            ),
                            ContentBlock::ToolResult { .. } => (
                                ResumedHistoryItemKind::ToolResult {
                                    message_index,
                                    block_index,
                                },
                                10,
                            ),
                            ContentBlock::Image { .. } => continue,
                        };
                        items.push(ResumedHistoryItem {
                            id: format!(
                                "history-{step_index}-message-{message_index}-block-{block_index}"
                            ),
                            estimated_lines,
                            step_index,
                            kind,
                        });
                    }
                }
            }
        }
        if step.finalize_cause.is_some() {
            items.push(ResumedHistoryItem {
                id: format!("history-{step_index}-terminal"),
                estimated_lines: 1,
                step_index,
                kind: ResumedHistoryItemKind::TerminalNotice,
            });
        }
    }
    items
}

impl ResumedHistoryStep {
    pub(crate) fn messages(&self) -> impl Iterator<Item = &Message> {
        self.message_segments
            .iter()
            .flat_map(|segment| segment.iter())
    }

    pub(crate) fn message(&self, index: usize) -> Option<&Message> {
        self.messages().nth(index)
    }
}
