use crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause;
use sdk::{
    LocalResumeContentBlock as ContentBlock, LocalResumeMessage as Message,
    LocalResumeMessageSource as MessageSource, LocalResumeRole as Role,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

const MAX_LOADED_HISTORY_STEPS: usize = 128;

#[derive(Clone, Debug)]
pub(crate) struct DisplayHistoryStepSlot {
    pub(crate) run_id: String,
    pub(crate) step_id: String,
    pub(crate) member_name: String,
    pub(crate) estimated_lines: usize,
    pub(crate) user_input_history: Vec<String>,
    pub(crate) finalize_cause: Option<TuiResumedStepFinalizeCause>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResumedHistoryStep {
    pub(crate) run_id: String,
    pub(crate) step_id: String,
    pub(crate) message_segments: Vec<Arc<[Message]>>,
    pub(crate) finalize_cause: Option<TuiResumedStepFinalizeCause>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TypedJsonHistorySource {
    SkillRequest,
    StopHook,
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
    TypedJson {
        message_index: usize,
        source: TypedJsonHistorySource,
        text: String,
    },
    StepPlaceholder,
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
    session_id: Option<String>,
    generation_revision: Option<u64>,
    step_slots: Vec<DisplayHistoryStepSlot>,
    steps: Vec<ResumedHistoryStep>,
    loaded_steps: HashMap<usize, ResumedHistoryStep>,
    loaded_order: VecDeque<usize>,
    items: Vec<ResumedHistoryItem>,
}

impl PartialEq for ResumedHistoryBacking {
    fn eq(&self, other: &Self) -> bool {
        self.steps.len() == other.steps.len()
            && self.step_slots.len() == other.step_slots.len()
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
        if let Some(index) = backing.display_history {
            return Self::from_index(index);
        }
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
        Self {
            steps,
            items,
            ..Self::default()
        }
    }

    pub(crate) fn from_index(index: sdk::DisplayHistoryIndex) -> Self {
        Self::from_tui_index(crate::tui::adapter::runtime_view::TuiDisplayHistoryIndex {
            session_id: index.session_id,
            generation_revision: index.generation_revision,
            steps: index
                .steps
                .into_iter()
                .map(
                    |step| crate::tui::adapter::runtime_view::TuiDisplayHistoryStepReference {
                        run_id: step.run_id,
                        step_id: step.step_id,
                        member_name: step.member_name,
                        estimated_lines: step.estimated_lines,
                        user_input_history: step.user_input_history,
                        finalize_cause: step.finalize_cause.map(map_finalize_cause),
                        duration_ms: step.duration_ms,
                    },
                )
                .collect(),
        })
    }

    pub(crate) fn from_tui_index(
        index: crate::tui::adapter::runtime_view::TuiDisplayHistoryIndex,
    ) -> Self {
        let step_slots = index
            .steps
            .into_iter()
            .map(|step| DisplayHistoryStepSlot {
                run_id: step.run_id,
                step_id: step.step_id,
                member_name: step.member_name,
                estimated_lines: step.estimated_lines,
                user_input_history: step.user_input_history,
                finalize_cause: step.finalize_cause,
                duration_ms: step.duration_ms,
            })
            .collect::<Vec<_>>();
        let items = step_slots
            .iter()
            .enumerate()
            .map(|(step_index, step)| ResumedHistoryItem {
                id: format!("history-step-{step_index}"),
                estimated_lines: step.estimated_lines,
                step_index,
                kind: ResumedHistoryItemKind::StepPlaceholder,
            })
            .collect();
        Self {
            session_id: Some(index.session_id),
            generation_revision: Some(index.generation_revision),
            step_slots,
            items,
            ..Self::default()
        }
    }

    pub(crate) fn requested_member_names(&self, item_ids: &[String]) -> Vec<String> {
        item_ids
            .iter()
            .filter_map(|item_id| self.item(item_id))
            .filter(|item| !self.loaded_steps.contains_key(&item.step_index))
            .filter_map(|item| self.step_slots.get(item.step_index))
            .map(|step| step.member_name.clone())
            .collect()
    }

    pub(crate) fn history_window_request(
        &self,
        item_ids: &[String],
    ) -> Option<sdk::DisplayHistoryWindowRequest> {
        let member_names = self.requested_member_names(item_ids);
        if member_names.is_empty() {
            return None;
        }
        Some(sdk::DisplayHistoryWindowRequest {
            session_id: self.session_id.clone()?,
            generation_revision: self.generation_revision?,
            member_names,
        })
    }

    pub(crate) fn apply_window(
        &mut self,
        window: crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow,
    ) -> bool {
        if self.session_id.as_deref() != Some(window.session_id.as_str())
            || self.generation_revision != Some(window.generation_revision)
        {
            return false;
        }
        for step in window.steps {
            let Some(step_index) = self
                .step_slots
                .iter()
                .position(|slot| slot.run_id == step.run_id && slot.step_id == step.step_id)
            else {
                continue;
            };
            let restored = resumed_step_from_tui(step);
            self.loaded_steps.insert(step_index, restored);
            self.replace_placeholder_items(step_index);
            self.loaded_order.retain(|loaded| *loaded != step_index);
            self.loaded_order.push_back(step_index);
        }
        while self.loaded_order.len() > MAX_LOADED_HISTORY_STEPS {
            if let Some(evicted) = self.loaded_order.pop_front() {
                self.loaded_steps.remove(&evicted);
                self.restore_step_placeholder(evicted);
            }
        }
        true
    }

    fn replace_placeholder_items(&mut self, step_index: usize) {
        let Some(step) = self.loaded_steps.get(&step_index) else {
            return;
        };
        let replacement = build_items_for_step(step_index, step);
        let Some(item_index) = self
            .items
            .iter()
            .position(|item| item.step_index == step_index)
        else {
            return;
        };
        self.items.splice(item_index..=item_index, replacement);
    }

    fn restore_step_placeholder(&mut self, step_index: usize) {
        let Some(slot) = self.step_slots.get(step_index) else {
            return;
        };
        let Some(item_start) = self
            .items
            .iter()
            .position(|item| item.step_index == step_index)
        else {
            return;
        };
        let item_end = self.items[item_start..]
            .iter()
            .take_while(|item| item.step_index == step_index)
            .count()
            .saturating_add(item_start);
        self.items.splice(
            item_start..item_end,
            [ResumedHistoryItem {
                id: format!("history-step-{step_index}"),
                estimated_lines: slot.estimated_lines,
                step_index,
                kind: ResumedHistoryItemKind::StepPlaceholder,
            }],
        );
    }

    pub(crate) fn loaded_step_count(&self) -> usize {
        self.loaded_steps.len()
    }

    pub(crate) fn steps(&self) -> &[ResumedHistoryStep] {
        &self.steps
    }

    pub(crate) fn step(&self, index: usize) -> Option<&ResumedHistoryStep> {
        self.loaded_steps
            .get(&index)
            .or_else(|| self.steps.get(index))
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
        if !self.step_slots.is_empty() {
            return self
                .step_slots
                .iter()
                .flat_map(|step| step.user_input_history.iter().cloned())
                .collect();
        }
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

fn map_finalize_cause(cause: sdk::ResumedStepFinalizeCause) -> TuiResumedStepFinalizeCause {
    match cause {
        sdk::ResumedStepFinalizeCause::Completed => TuiResumedStepFinalizeCause::Completed,
        sdk::ResumedStepFinalizeCause::UserCancelledStep => {
            TuiResumedStepFinalizeCause::UserCancelledStep
        }
        sdk::ResumedStepFinalizeCause::RunTerminated => TuiResumedStepFinalizeCause::RunTerminated,
    }
}

fn resumed_step_from_wire(step: sdk::ResumedSessionStep) -> ResumedHistoryStep {
    let local = sdk::LocalResumedSessionStep::from_wire(step);
    ResumedHistoryStep {
        run_id: local.run_id,
        step_id: local.step_id,
        message_segments: local.message_segments,
        finalize_cause: local.finalize_cause.map(map_finalize_cause),
        duration_ms: local.duration_ms,
    }
}

fn local_message_from_tui(message: crate::tui::adapter::runtime_view::TuiChatMessage) -> Message {
    let metadata = match message.source {
        crate::tui::adapter::runtime_view::TuiMessageSource::User => None,
        crate::tui::adapter::runtime_view::TuiMessageSource::SystemGenerated => {
            Some(sdk::ChatMessageMetadata {
                source: sdk::ChatMessageSource::SystemGenerated,
                stop_hook: None,
                skill_request: None,
            })
        }
        crate::tui::adapter::runtime_view::TuiMessageSource::StopHook => {
            Some(sdk::ChatMessageMetadata {
                source: sdk::ChatMessageSource::StopHook,
                stop_hook: message.stop_hook.map(|feedback| sdk::StopHookFeedbackView {
                    summary: feedback.summary,
                    command: feedback.command,
                    exit_code: feedback.exit_code,
                    reason: feedback.reason,
                    stdout_preview: feedback.stdout_preview,
                    stderr_preview: feedback.stderr_preview,
                    stdout_truncated: feedback.stdout_truncated,
                    stderr_truncated: feedback.stderr_truncated,
                    output_file: feedback.output_file,
                }),
                skill_request: None,
            })
        }
        crate::tui::adapter::runtime_view::TuiMessageSource::SkillRequest => {
            Some(sdk::ChatMessageMetadata {
                source: sdk::ChatMessageSource::SkillRequest,
                stop_hook: None,
                skill_request: message
                    .skill_request
                    .map(|request| sdk::SkillRequestMetadataView {
                        skill: request.skill,
                        arguments: request.arguments,
                        raw_input: request.raw_input,
                    }),
            })
        }
    };
    let wire = sdk::ChatMessage {
        role: message.role,
        content: message
            .content
            .into_iter()
            .map(|block| match block {
                crate::tui::adapter::runtime_view::TuiContentBlock::Text { text } => {
                    sdk::ContentBlock::Text { text }
                }
                crate::tui::adapter::runtime_view::TuiContentBlock::Image {
                    media_type,
                    base64,
                    placeholder,
                } => sdk::ContentBlock::Image {
                    source: sdk::ImageSource::Base64 {
                        media_type,
                        data: base64,
                    },
                    placeholder,
                },
                crate::tui::adapter::runtime_view::TuiContentBlock::ToolUse { id, name, input } => {
                    sdk::ContentBlock::ToolUse { id, name, input }
                }
                crate::tui::adapter::runtime_view::TuiContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    text,
                } => sdk::ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    text,
                },
                crate::tui::adapter::runtime_view::TuiContentBlock::Thinking {
                    thinking,
                    signature,
                } => sdk::ContentBlock::Thinking {
                    thinking,
                    signature,
                },
            })
            .collect(),
        metadata,
        input_id: message
            .input_id
            .and_then(|id| sdk::InputId::parse_uuid7(&id).ok()),
    };
    sdk::LocalResumedSessionStep::from_wire(sdk::ResumedSessionStep {
        run_id: String::new(),
        step_id: String::new(),
        messages: vec![wire],
        finalize_cause: None,
        duration_ms: None,
    })
    .message_segments
    .into_iter()
    .next()
    .and_then(|messages| messages.first().cloned())
    .unwrap_or_else(|| Message::user(""))
}

fn resumed_step_from_tui(
    step: crate::tui::adapter::runtime_view::TuiResumedSessionStep,
) -> ResumedHistoryStep {
    let messages = step
        .messages
        .into_iter()
        .map(local_message_from_tui)
        .collect::<Vec<_>>();
    ResumedHistoryStep {
        run_id: step.run_id,
        step_id: step.step_id,
        message_segments: vec![messages.into()],
        finalize_cause: step.finalize_cause,
        duration_ms: step.duration_ms,
    }
}

fn build_items(steps: &[ResumedHistoryStep]) -> Vec<ResumedHistoryItem> {
    steps
        .iter()
        .enumerate()
        .flat_map(|(step_index, step)| build_items_for_step(step_index, step))
        .collect()
}

fn build_items_for_step(step_index: usize, step: &ResumedHistoryStep) -> Vec<ResumedHistoryItem> {
    let mut items = Vec::new();
    for (message_index, message) in step.messages().enumerate() {
        match message.role {
            Role::User
                if message.source() == MessageSource::User && !message.has_tool_results() =>
            {
                let text = message.text_content();
                items.push(ResumedHistoryItem {
                    id: format!("history-{step_index}-message-{message_index}"),
                    estimated_lines: text.lines().count().max(1).saturating_add(2),
                    step_index,
                    kind: ResumedHistoryItemKind::UserMessage { message_index },
                });
            }
            Role::User
                if message.source() == MessageSource::SkillRequest
                    && !message.has_tool_results() =>
            {
                if let Some(payload) = message
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.skill_request.as_ref())
                {
                    let text = serde_json::to_string_pretty(payload).unwrap_or_default();
                    items.push(ResumedHistoryItem {
                        id: format!("history-{step_index}-message-{message_index}-skill-request"),
                        estimated_lines: text.lines().count().max(1).saturating_add(1),
                        step_index,
                        kind: ResumedHistoryItemKind::TypedJson {
                            message_index,
                            source: TypedJsonHistorySource::SkillRequest,
                            text,
                        },
                    });
                }
            }
            Role::User
                if message.source() == MessageSource::StopHook && !message.has_tool_results() =>
            {
                let text = message
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.stop_hook.as_ref())
                    .and_then(|payload| serde_json::to_string_pretty(payload).ok())
                    .unwrap_or_else(|| message.text_content());
                items.push(ResumedHistoryItem {
                    id: format!("history-{step_index}-message-{message_index}-stop-hook"),
                    estimated_lines: text.lines().count().max(1).saturating_add(1),
                    step_index,
                    kind: ResumedHistoryItemKind::TypedJson {
                        message_index,
                        source: TypedJsonHistorySource::StopHook,
                        text,
                    },
                });
            }
            Role::User
                if message.source() != MessageSource::User && !message.has_tool_results() => {}
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
