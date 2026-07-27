use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime_presentation::RuntimePresentationIntent;
use crate::tui::model::ui_preferences::UiPreferencesIntent;
use crate::tui::model::workspace_provider::WorkspaceIntent;

#[derive(Clone, Debug, PartialEq)]
pub enum AgentIntent {
    Conversation(ConversationIntent),
    RuntimePresentation(RuntimePresentationIntent),
    UiPreferences(UiPreferencesIntent),
    Input(InputIntent),
    Diagnostic(DiagnosticIntent),
    Session(SessionIntent),
    Workspace(WorkspaceIntent),
}
