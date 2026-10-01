use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::change::ConversationChange;
#[cfg(test)]
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
        _ => Vec::new(),
    }
}

#[cfg(test)]
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
#[path = "coordinator_tests.rs"]
mod tests;
