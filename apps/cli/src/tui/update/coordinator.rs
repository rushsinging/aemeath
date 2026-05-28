use crate::tui::effect::effect::Effect;
use crate::tui::model::input::change::InputChange;

pub fn effects_for_input_change(change: &InputChange) -> Vec<Effect> {
    match change {
        InputChange::TextChanged { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::AttachmentChanged { .. }
        | InputChange::ModeChanged { .. }
        | InputChange::Submitted { .. }
        | InputChange::Cleared => vec![Effect::RequestRender],
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::effect::effect::Effect;
    use crate::tui::model::input::change::InputChange;
    use crate::tui::model::input::submission::InputSubmission;

    use super::effects_for_input_change;

    #[test]
    fn test_submitted_input_requests_render() {
        let effects = effects_for_input_change(&InputChange::Submitted {
            submission: InputSubmission {
                text: "hello".to_string(),
                attachments: Vec::new(),
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
