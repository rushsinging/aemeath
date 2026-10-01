use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::change::ConversationChange;
use crate::tui::model::conversation::interaction::UiInteractionRequestId;
use crate::tui::model::input::change::InputChange;
use crate::tui::model::input::submission::InputSubmission;

use super::effects_for_input_change;

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
