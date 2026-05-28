use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::input::model::InputModel;
use crate::tui::model::runtime::model::RuntimeModel;
use crate::tui::model::runtime::session_model::SessionModel;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TuiModel {
    pub conversation: ConversationModel,
    pub diagnostic: DiagnosticModel,
    pub input: InputModel,
    pub runtime: RuntimeModel,
    pub session: SessionModel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_model_default_has_no_active_chat() {
        let model = TuiModel::default();
        assert!(model.conversation.active_chat_id.is_none());
    }

    #[test]
    fn test_tui_model_default_has_no_prompt() {
        let model = TuiModel::default();
        assert!(model.diagnostic.active_prompt.is_none());
    }

    #[test]
    fn test_tui_model_default_has_empty_input() {
        let model = TuiModel::default();
        assert!(model.input.document.buffer.is_empty());
    }
}
