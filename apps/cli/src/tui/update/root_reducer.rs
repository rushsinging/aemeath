use crate::tui::adapter::agent_event::{map_agent_event, AgentEventMapping};
use crate::tui::adapter::effect_result::{map_effect_result, EffectResultMapping};
use crate::tui::adapter::input::{route_submission, ConversationAvailability, SubmissionRoute};
use crate::tui::adapter::key_event::map_key_event;
use crate::tui::effect::effect::Effect;
use crate::tui::model::change::{dirty_from_model_changes, ModelChange};
use crate::tui::model::conversation::change::ConversationChange;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::input::change::InputChange;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::root::TuiModel;
use crate::tui::update::msg::TuiMsg;
use crate::tui::view_state::{AppViewState, ViewModelDirty};

#[derive(Debug, Default, PartialEq)]
pub struct TuiUpdateResult {
    pub dirty: ViewModelDirty,
    pub effects: Vec<Effect>,
}

impl TuiUpdateResult {
    pub(crate) fn push_render_request_once(&mut self) {
        if !self.effects.contains(&Effect::RequestRender) {
            self.effects.push(Effect::RequestRender);
        }
    }

    pub(crate) fn dedupe_render_requests(&mut self) {
        let mut seen = false;
        self.effects.retain(|effect| {
            if !matches!(effect, Effect::RequestRender) {
                return true;
            }
            if seen {
                false
            } else {
                seen = true;
                true
            }
        });
    }
}

pub fn update(model: &mut TuiModel, view_state: &mut AppViewState, msg: TuiMsg) -> TuiUpdateResult {
    let result = match msg {
        TuiMsg::TerminalKey(key) => reduce_key(model, key),
        TuiMsg::TerminalResize { width, height } => {
            view_state.layout.terminal_width = width;
            view_state.layout.terminal_height = height;
            let mut dirty = ViewModelDirty::default();
            dirty.mark_all();
            TuiUpdateResult {
                dirty,
                effects: vec![Effect::RequestRender],
            }
        }
        TuiMsg::AgentEvent(event) => reduce_agent_event(model, map_agent_event(&event)),
        TuiMsg::EffectCompleted(result) => reduce_effect_result(model, map_effect_result(result)),
        TuiMsg::RenderTick
        | TuiMsg::TimerTick { .. }
        | TuiMsg::TerminalMouse(_)
        | TuiMsg::Key(_)
        | TuiMsg::Mouse(_)
        | TuiMsg::Paste(_)
        | TuiMsg::Resize { .. }
        | TuiMsg::SpinnerTick
        | TuiMsg::Ui(_) => TuiUpdateResult {
            effects: vec![Effect::RequestRender],
            ..TuiUpdateResult::default()
        },
    };
    crate::tui::update::dirty::merge_dirty(&mut view_state.dirty, result.dirty.clone());
    result
}

fn reduce_key(model: &mut TuiModel, key: crossterm::event::KeyEvent) -> TuiUpdateResult {
    let mapping = map_key_event(key);
    if mapping.quit_requested {
        return TuiUpdateResult::default();
    }

    let mut result = TuiUpdateResult::default();
    for intent in mapping.input {
        let intent = rewrite_history_to_completion(&model.input, intent);
        let changes = model.input.apply(intent);
        apply_input_changes(&mut result, &changes);
    }

    if mapping.submit_requested {
        let changes = model.input.apply(InputIntent::Submit);
        let submission = changes.iter().find_map(|change| match change {
            InputChange::Submitted { submission } => Some(submission.clone()),
            _ => None,
        });
        apply_input_changes(&mut result, &changes);
        if let Some(submission) = submission {
            let availability = if model.conversation.active_chat_id.is_some() {
                ConversationAvailability::Running
            } else {
                ConversationAvailability::Idle
            };
            match route_submission(
                submission,
                availability,
                model.diagnostic.active_prompt.is_some(),
            ) {
                SubmissionRoute::StartChat { submission } => {
                    let changes =
                        model
                            .conversation
                            .apply(ConversationIntent::StartChat(StartChat {
                                submission: submission.text.clone(),
                            }));
                    apply_conversation_changes(
                        &mut result,
                        &changes,
                        &mut model.conversation.runtime,
                    );
                    result.effects.push(Effect::SpawnAgentChat {
                        chat_id: model
                            .conversation
                            .active_chat_id
                            .as_ref()
                            .map(|id| id.as_ref().to_string())
                            .unwrap_or_default(),
                        prompt: submission.text,
                    });
                }
                SubmissionRoute::QueueSubmission { submission } => {
                    let changes = model
                        .conversation
                        .apply(ConversationIntent::QueueSubmission(QueueSubmission {
                            input_id: sdk::InputId::new_v7(),
                            text: submission.text,
                        }));
                    apply_conversation_changes(
                        &mut result,
                        &changes,
                        &mut model.conversation.runtime,
                    );
                }
                SubmissionRoute::AnswerPrompt { text } => {
                    model.diagnostic.apply(
                        crate::tui::model::diagnostic::intent::DiagnosticIntent::AnswerPrompt {
                            answer: text,
                        },
                    );
                    result.dirty.mark_dialog();
                }
            }
        }
    }
    result
}

pub(crate) fn reduce_agent_event(
    model: &mut TuiModel,
    mapping: AgentEventMapping,
) -> TuiUpdateResult {
    let mut result = TuiUpdateResult::default();
    for intent in mapping.conversation {
        let changes = model.conversation.apply(intent);
        apply_conversation_changes(&mut result, &changes, &mut model.conversation.runtime);
    }
    for intent in mapping.diagnostic {
        model.diagnostic.apply(intent);
        result.dirty.mark_status();
        result.dirty.mark_dialog();
    }
    for intent in mapping.session {
        model.session.apply(intent);
        result.dirty.mark_status();
    }
    result.effects.extend(mapping.effects);
    result.dedupe_render_requests();
    if result.dirty.output || result.dirty.status || result.dirty.dialog {
        result.push_render_request_once();
    }
    result
}

fn reduce_effect_result(model: &mut TuiModel, mapping: EffectResultMapping) -> TuiUpdateResult {
    let mut result = TuiUpdateResult::default();
    for intent in mapping.diagnostic {
        model.diagnostic.apply(intent);
        result.dirty.mark_status();
    }
    for intent in mapping.session {
        model.session.apply(intent);
        result.dirty.mark_status();
    }
    result
}

/// 补全弹窗可见时，将 Up/Down 重写为补全选择。
fn rewrite_history_to_completion(
    input: &crate::tui::model::input::model::InputModel,
    intent: InputIntent,
) -> InputIntent {
    if input.completion.visible {
        match intent {
            InputIntent::MoveCursorUp | InputIntent::MoveHistoryPrevious => {
                InputIntent::SelectCompletionPrevious
            }
            InputIntent::MoveCursorDown | InputIntent::MoveHistoryNext => {
                InputIntent::SelectCompletionNext
            }
            other => other,
        }
    } else {
        intent
    }
}

fn apply_input_changes(result: &mut TuiUpdateResult, changes: &[InputChange]) {
    if changes.is_empty() {
        return;
    }
    result.dirty.mark_input();
    result.push_render_request_once();
}

fn apply_conversation_changes(
    result: &mut TuiUpdateResult,
    changes: &[ConversationChange],
    runtime: &mut crate::tui::model::conversation::runtime_state::RuntimeState,
) {
    // change→RuntimeState 映射层：对话域 change 驱动运行态转换。
    // intent_impls 不再直接操作 runtime，spinner 等副作用由此层统一处理。
    for change in changes {
        match change {
            ConversationChange::ChatStarted { .. } => runtime.start_chat(),
            ConversationChange::ChatCompleted { .. }
            | ConversationChange::ChatCompleting { .. } => runtime.complete_chat(),
            ConversationChange::AssistantTextAppended { .. } => runtime.generate(),
            ConversationChange::ThinkingTextAppended { .. } => runtime.think(),
            ConversationChange::ToolCallObserved { name, .. } => runtime.start_tool_call(name),
            ConversationChange::ToolCallCompleted { .. } => runtime.complete_tool_call(),
            ConversationChange::ErrorAppended { .. } => runtime.abort_chat(),
            ConversationChange::AgentProgressRecorded { .. } => runtime.report_agent_progress(),
            ConversationChange::AskUserShown { .. } => runtime.pause_chat(),
            ConversationChange::AskUserUpdated { .. } | ConversationChange::AskUserDismissed => {
                runtime.resume_chat()
            }
            _ => {}
        }
    }
    let model_changes: Vec<ModelChange> = changes.iter().map(ModelChange::from).collect();
    let dirty = dirty_from_model_changes(&model_changes);
    result.dirty.merge(&dirty);
    if dirty.output || dirty.status {
        result.push_render_request_once();
    }
}

impl From<&ConversationChange> for ModelChange {
    fn from(change: &ConversationChange) -> Self {
        match change {
            ConversationChange::OutputDirty
            | ConversationChange::UserMessageAppended { .. }
            | ConversationChange::AssistantTextAppended { .. }
            | ConversationChange::ThinkingTextAppended { .. }
            | ConversationChange::ToolCallObserved { .. }
            | ConversationChange::ToolCallBound { .. }
            | ConversationChange::ToolCallCompleted { .. }
            | ConversationChange::OrphanToolResultObserved { .. }
            | ConversationChange::SystemMessageAppended { .. }
            | ConversationChange::ErrorAppended { .. }
            | ConversationChange::QueuedSubmissionAdded { .. }
            | ConversationChange::QueuedSubmissionsCleared { .. }
            | ConversationChange::AgentProgressRecorded { .. }
            | ConversationChange::BlockCompleted { .. }
            | ConversationChange::AskUserShown { .. }
            | ConversationChange::AskUserUpdated { .. }
            | ConversationChange::AskUserDismissed
            | ConversationChange::StyleBoundaryResetRequired => ModelChange::output_dirty(),
            ConversationChange::ChatStarted { .. }
            | ConversationChange::ChatTurnStarted { .. }
            | ConversationChange::ChatCompleting { .. }
            | ConversationChange::ChatCompleted { .. }
            | ConversationChange::ProviderModelChanged { .. }
            | ConversationChange::WorkspaceChanged { .. }
            | ConversationChange::WorkspaceSnapshotChanged { .. }
            | ConversationChange::UsageChanged { .. }
            | ConversationChange::LiveTpsChanged { .. }
            | ConversationChange::TaskStatusChanged { .. }
            | ConversationChange::ProcessingJobChanged { .. }
            | ConversationChange::SpinnerPhaseChanged
            | ConversationChange::SpinnerStopped
            | ConversationChange::TaskLinesChanged
            | ConversationChange::StatusNoticeChanged
            | ConversationChange::ThinkingChanged
            | ConversationChange::GraphPhaseChanged => ModelChange::status_dirty(),
        }
    }
}

#[cfg(test)]
#[path = "root_reducer_tests.rs"]
mod tests;
