use crate::tui::adapter::agent_event::{map_agent_event, AgentEventMapping};
use crate::tui::adapter::effect_result::{map_effect_result, EffectResultMapping};
use crate::tui::adapter::input::{route_submission, ConversationAvailability, SubmissionRoute};
use crate::tui::adapter::key_event::map_key_event;
use crate::tui::adapter::resize::{apply_resize, map_resize};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::change::ConversationChange;
use crate::tui::model::conversation::intent::ConversationIntent;
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

pub fn update(model: &mut TuiModel, view_state: &mut AppViewState, msg: TuiMsg) -> TuiUpdateResult {
    match msg {
        TuiMsg::TerminalKey(key) => reduce_key(model, key),
        TuiMsg::TerminalResize { width, height } => {
            apply_resize(view_state, map_resize(width, height));
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
    }
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
                    let changes = model.conversation.apply(ConversationIntent::StartChat {
                        submission: submission.text.clone(),
                    });
                    apply_conversation_changes(&mut result, &changes);
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
                        .apply(ConversationIntent::QueueSubmission {
                            text: submission.text,
                        });
                    apply_conversation_changes(&mut result, &changes);
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
        apply_conversation_changes(&mut result, &changes);
    }
    for intent in mapping.diagnostic {
        model.diagnostic.apply(intent);
        result.dirty.mark_status();
        result.dirty.mark_dialog();
    }
    for intent in mapping.runtime {
        model.runtime.apply(intent);
        result.dirty.mark_status();
    }
    for intent in mapping.session {
        model.session.apply(intent);
        result.dirty.mark_status();
    }
    result.effects.extend(mapping.effects);
    if result.dirty.output || result.dirty.status || result.dirty.dialog {
        result.effects.push(Effect::RequestRender);
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

/// 补全弹窗可见时，将 Up/Down 历史导航重写为补全选择。
fn rewrite_history_to_completion(
    input: &crate::tui::model::input::model::InputModel,
    intent: InputIntent,
) -> InputIntent {
    if input.completion.visible {
        match intent {
            InputIntent::MoveHistoryPrevious => InputIntent::SelectCompletionPrevious,
            InputIntent::MoveHistoryNext => InputIntent::SelectCompletionNext,
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
    result.effects.push(Effect::RequestRender);
}

fn apply_conversation_changes(result: &mut TuiUpdateResult, changes: &[ConversationChange]) {
    for change in changes {
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
            | ConversationChange::TextBlockCompleted { .. }
            | ConversationChange::AskUserShown { .. }
            | ConversationChange::AskUserUpdated { .. }
            | ConversationChange::AskUserDismissed
            | ConversationChange::StyleBoundaryResetRequired => result.dirty.mark_output(),
            ConversationChange::ChatStarted { .. }
            | ConversationChange::ChatTurnStarted { .. }
            | ConversationChange::ChatCompleting { .. }
            | ConversationChange::ChatCompleted { .. } => result.dirty.mark_status(),
        }
    }
    if result.dirty.output || result.dirty.status {
        result.effects.push(Effect::RequestRender);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> TuiMsg {
        TuiMsg::TerminalKey(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn test_update_key_enter_idle_spawns_chat() {
        let mut model = TuiModel::default();
        let mut view_state = AppViewState::default();
        update(&mut model, &mut view_state, key(KeyCode::Char('h')));
        let result = update(&mut model, &mut view_state, key(KeyCode::Enter));
        assert!(matches!(
            result.effects.last(),
            Some(Effect::SpawnAgentChat { .. })
        ));
    }

    #[test]
    fn test_update_key_enter_running_queues_submission() {
        let mut model = TuiModel::default();
        model.conversation.apply(ConversationIntent::StartChat {
            submission: "old".to_string(),
        });
        let mut view_state = AppViewState::default();
        update(&mut model, &mut view_state, key(KeyCode::Char('n')));
        update(&mut model, &mut view_state, key(KeyCode::Enter));
        assert_eq!(model.conversation.queued_submissions.len(), 1);
    }

    #[test]
    fn test_update_agent_text_marks_output_dirty() {
        let mut model = TuiModel::default();
        let mut view_state = AppViewState::default();
        let result = update(
            &mut model,
            &mut view_state,
            TuiMsg::AgentEvent(crate::tui::app::event::UiEvent::Text("hi".into())),
        );
        assert!(result.dirty.output);
    }

    #[test]
    fn test_reduce_agent_event_tool_call_updates_conversation() {
        let mut model = TuiModel::default();
        model.conversation.apply(ConversationIntent::StartChat {
            submission: "read".to_string(),
        });
        reduce_agent_event(
            &mut model,
            AgentEventMapping {
                conversation: vec![ConversationIntent::ObserveToolCallStart {
                    name: "Read".to_string(),
                    index: 0,
                }],
                ..Default::default()
            },
        );
        reduce_agent_event(
            &mut model,
            AgentEventMapping {
                conversation: vec![ConversationIntent::ObserveToolCall {
                    id: "tool-1".to_string(),
                    name: "Read".to_string(),
                    index: 0,
                    summary: "Read file".to_string(),
                }],
                ..Default::default()
            },
        );

        assert!(model.conversation.blocks.iter().any(|block| matches!(
            block,
            crate::tui::model::conversation::block::ConversationBlock::ToolCall { id, .. }
                if id.as_ref() == "tool-1"
        )));
    }

    #[test]
    fn test_up_key_selects_completion_when_visible() {
        use crate::tui::model::input::completion_item::CompletionItem;

        let mut model = TuiModel::default();
        model.input.apply(InputIntent::InsertChar('/'));
        model.input.apply(InputIntent::SetCompletions {
            query: "/".to_string(),
            items: vec![
                CompletionItem::new("/help", "/help"),
                CompletionItem::new("/exit", "/exit"),
            ],
        });
        assert!(model.input.completion.visible);
        assert_eq!(model.input.completion.selected_index, Some(0));

        let mut view_state = AppViewState::default();
        update(&mut model, &mut view_state, key(KeyCode::Down));
        assert_eq!(
            model.input.completion.selected_index,
            Some(1),
            "Down 在补全可见时应选择下一项"
        );

        update(&mut model, &mut view_state, key(KeyCode::Up));
        assert_eq!(
            model.input.completion.selected_index,
            Some(0),
            "Up 在补全可见时应选择上一项"
        );
    }

    #[test]
    fn test_up_down_history_when_completion_hidden() {
        let mut model = TuiModel::default();
        model.input.apply(InputIntent::ReplaceHistory(vec![
            "first".to_string(),
            "second".to_string(),
        ]));
        assert!(!model.input.completion.visible);

        let mut view_state = AppViewState::default();
        update(&mut model, &mut view_state, key(KeyCode::Up));
        assert_eq!(
            model.input.document.buffer, "second",
            "Up 在补全不可见时应翻历史"
        );
    }
}
