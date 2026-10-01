use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::display_history::DisplayHistoryModel;
use crate::tui::model::input::model::InputModel;
use crate::tui::model::runtime::session_model::SessionModel;
use crate::tui::model::runtime_presentation::RuntimePresentation;
use crate::tui::model::ui_preferences::UiPreferences;
use crate::tui::model::workspace_provider::WorkspaceProvider;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TuiModel {
    pub conversation: ConversationModel,
    pub(crate) display_history: DisplayHistoryModel,
    pub runtime_presentation: RuntimePresentation,
    pub ui_preferences: UiPreferences,
    pub diagnostic: DiagnosticModel,
    pub input: InputModel,
    pub session: SessionModel,
    pub workspace_provider: WorkspaceProvider,
}

#[cfg(test)]
#[path = "root_tests.rs"]
mod tests;
