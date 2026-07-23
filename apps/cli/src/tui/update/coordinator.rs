use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::change::ConversationChange;
use crate::tui::model::input::change::InputChange;

use crate::tui::model::workspace_provider::WorkspaceChange;

pub fn effects_for_workspace_change(change: &WorkspaceChange) -> Vec<Effect> {
    match change {
        WorkspaceChange::SnapshotApplied {
            root: Some(root),
            revision,
        } => vec![Effect::ResolveWorkspaceMetadata {
            root: root.clone(),
            revision: *revision,
        }],
        _ => Vec::new(),
    }
}

pub fn effects_for_conversation_change(change: &ConversationChange) -> Vec<Effect> {
    match change {
        ConversationChange::ErrorAppended { message, .. } => vec![Effect::RunHook {
            name: "error".to_string(),
            message: message.clone(),
        }],
        ConversationChange::InteractionReplyRequested { request_id, reply } => {
            vec![Effect::ReplyInteraction {
                request_id: request_id.clone(),
                reply: reply.clone(),
            }]
        }
        ConversationChange::InteractionCancelRequested { request_id } => {
            vec![Effect::CancelInteraction {
                request_id: request_id.clone(),
                reason: crate::tui::model::conversation::interaction::UiInteractionCancelReason::UserCancelled,
            }]
        }
        _ => Vec::new(),
    }
}

pub fn effects_for_input_change(change: &InputChange) -> Vec<Effect> {
    match change {
        InputChange::TextChanged { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::ModeChanged { .. }
        | InputChange::Submitted { .. }
        | InputChange::Cleared => vec![Effect::RequestRender],
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::effect::effect::Effect;
    use crate::tui::model::conversation::change::ConversationChange;
    use crate::tui::model::conversation::interaction::{
        UiInteractionReply, UiInteractionRequestId,
    };
    use crate::tui::model::input::change::InputChange;
    use crate::tui::model::input::submission::InputSubmission;

    use super::effects_for_input_change;

    #[test]
    fn test_interaction_reply_change_emits_typed_effect() {
        let request_id = UiInteractionRequestId::from("request-1");
        let effects = super::effects_for_conversation_change(
            &ConversationChange::InteractionReplyRequested {
                request_id: request_id.clone(),
                reply: UiInteractionReply::ContinueHardPause,
            },
        );

        assert_eq!(
            effects,
            vec![Effect::ReplyInteraction {
                request_id,
                reply: UiInteractionReply::ContinueHardPause,
            }]
        );
    }

    #[test]
    fn test_interaction_display_changes_do_not_emit_command_effects() {
        assert!(
            super::effects_for_conversation_change(&ConversationChange::InteractionShown {
                request_id: UiInteractionRequestId::from("request-1"),
            })
            .is_empty()
        );
    }

    #[test]
    fn test_submitted_input_requests_render() {
        let effects = effects_for_input_change(&InputChange::Submitted {
            submission: InputSubmission {
                text: "hello".to_string(),
                display_text: "hello".to_string(),
                images: Vec::new(),
            },
        });
        assert!(effects.contains(&Effect::RequestRender));
    }

    #[test]
    fn test_text_changed_requests_render() {
        let effects = effects_for_input_change(&InputChange::TextChanged {
            text: "hello".to_string(),
            cursor: 5,
        });
        assert_eq!(effects, vec![Effect::RequestRender]);
    }

    #[test]
    fn test_cleared_requests_render() {
        let effects = effects_for_input_change(&InputChange::Cleared);
        assert_eq!(effects, vec![Effect::RequestRender]);
    }
}
