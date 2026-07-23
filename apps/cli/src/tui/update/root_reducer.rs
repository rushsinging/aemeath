use crate::tui::adapter::agent_event::AgentEventMapping;
use crate::tui::effect::effect::Effect;
use crate::tui::model::change::{dirty_from_model_changes, ModelChange};
use crate::tui::model::conversation::change::ConversationChange;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::root::TuiModel;
use crate::tui::update::intent::AgentIntent;
use crate::tui::view_state::ViewModelDirty;

#[derive(Debug, Default, PartialEq)]
pub struct TuiUpdateResult {
    pub dirty: ViewModelDirty,
    pub effects: Vec<Effect>,
    pub input_changes: Vec<crate::tui::model::input::change::InputChange>,
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

pub(crate) fn reduce_intent(model: &mut TuiModel, intent: AgentIntent) -> TuiUpdateResult {
    let mut result = TuiUpdateResult::default();

    match intent {
        AgentIntent::Conversation(ConversationIntent::ResumeConversation(intent)) => {
            let changes = model
                .conversation
                .apply(ConversationIntent::ResumeConversation(intent));
            apply_history_conversation_changes(&mut result, &changes);
            model.conversation.runtime.force_idle();
            result.dirty.mark_status();
        }
        AgentIntent::Conversation(intent) => {
            let changes = model.conversation.apply(intent);
            apply_conversation_changes(&mut result, &changes, &mut model.conversation.runtime);
        }
        AgentIntent::Config(intent) => {
            model.config_provider.apply(intent);
            result.dirty.mark_status();
        }
        AgentIntent::Input(intent) => {
            result.input_changes = model.input.apply(intent);
            result.dirty.mark_input();
        }
        AgentIntent::Diagnostic(intent) => {
            model.diagnostic.apply(intent);
            result.dirty.mark_status();
            result.dirty.mark_dialog();
        }
        AgentIntent::Session(intent) => {
            model.session.apply(intent);
            result.dirty.mark_status();
        }
        AgentIntent::Workspace(intent) => {
            let change = model.workspace_provider.apply(intent);
            apply_workspace_change(&mut result, change);
        }
    }

    result.dedupe_render_requests();
    if result.dirty.output || result.dirty.status || result.dirty.input || result.dirty.dialog {
        result.push_render_request_once();
    }
    result
}

fn apply_workspace_change(
    result: &mut TuiUpdateResult,
    change: crate::tui::model::workspace_provider::WorkspaceChange,
) {
    result
        .effects
        .extend(crate::tui::update::coordinator::effects_for_workspace_change(&change));
    match change {
        crate::tui::model::workspace_provider::WorkspaceChange::CurrentChanged => {
            result.dirty.mark_status();
        }
        crate::tui::model::workspace_provider::WorkspaceChange::SnapshotApplied { .. } => {
            result.dirty.mark_status();
            result.dirty.mark_output();
        }
        crate::tui::model::workspace_provider::WorkspaceChange::MetadataApplied { .. } => {
            result.dirty.mark_status();
        }
        crate::tui::model::workspace_provider::WorkspaceChange::MetadataDiscarded { .. } => {}
    }
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
    for intent in mapping.workspace {
        let change = model.workspace_provider.apply(intent);
        apply_workspace_change(&mut result, change);
    }
    result.dedupe_render_requests();
    if result.dirty.output || result.dirty.status || result.dirty.dialog {
        result.push_render_request_once();
    }
    result
}

fn apply_history_conversation_changes(
    result: &mut TuiUpdateResult,
    changes: &[ConversationChange],
) {
    for change in changes {
        result
            .effects
            .extend(crate::tui::update::coordinator::effects_for_conversation_change(change));
    }
    let model_changes: Vec<ModelChange> = changes.iter().map(ModelChange::from).collect();
    let dirty = dirty_from_model_changes(&model_changes);
    result.dirty.merge(&dirty);
    if dirty.output || dirty.status {
        result.push_render_request_once();
    }
}

fn apply_conversation_changes(
    result: &mut TuiUpdateResult,
    changes: &[ConversationChange],
    runtime: &mut crate::tui::model::conversation::runtime_state::RuntimeState,
) {
    for change in changes {
        result
            .effects
            .extend(crate::tui::update::coordinator::effects_for_conversation_change(change));
        match change {
            ConversationChange::ChatStarted { .. } => runtime.start_chat(),
            ConversationChange::ChatCompleted { .. }
            | ConversationChange::ChatCompleting { .. } => runtime.complete_chat(),
            ConversationChange::AssistantTextAppended { .. } => runtime.generate(),
            ConversationChange::ThinkingTextAppended { .. } => runtime.think(),
            ConversationChange::ToolCallBound { name, running, .. } if *running => {
                runtime.start_tool_call(name)
            }
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
            | ConversationChange::AgentMetaUpdated { .. }
            | ConversationChange::BlockCompleted { .. }
            | ConversationChange::AskUserShown { .. }
            | ConversationChange::AskUserUpdated { .. }
            | ConversationChange::AskUserDismissed
            | ConversationChange::InteractionShown { .. }
            | ConversationChange::InteractionUpdated { .. }
            | ConversationChange::InteractionReplyRequested { .. }
            | ConversationChange::InteractionCancelRequested { .. }
            | ConversationChange::InteractionCompleted { .. }
            | ConversationChange::InteractionCommandRejected { .. }
            | ConversationChange::InteractionConflict { .. }
            | ConversationChange::AgentRunChanged { .. }
            | ConversationChange::AgentRunStepChanged { .. } => {
                ModelChange::output_and_status_dirty()
            }
            ConversationChange::CompactProgressChanged
            | ConversationChange::QueuedSubmissionsSynced { .. }
            | ConversationChange::CompactRuntimeCleared
            | ConversationChange::StyleBoundaryResetRequired => ModelChange::output_dirty(),
            ConversationChange::ChatStarted { .. }
            | ConversationChange::ChatTurnStarted { .. }
            | ConversationChange::ChatCompleting { .. }
            | ConversationChange::ChatCompleted { .. }
            | ConversationChange::UsageChanged { .. }
            | ConversationChange::LiveTpsChanged { .. }
            | ConversationChange::TaskStatusChanged { .. }
            | ConversationChange::ProcessingJobChanged { .. }
            | ConversationChange::SpinnerPhaseChanged
            | ConversationChange::SpinnerStopped
            | ConversationChange::TaskLinesChanged
            | ConversationChange::StatusNoticeChanged
            | ConversationChange::GraphPhaseChanged => ModelChange::status_dirty(),
        }
    }
}

#[cfg(test)]
#[path = "root_reducer_intent_tests.rs"]
mod intent_tests;

#[cfg(test)]
#[path = "root_reducer_tests.rs"]
mod tests;
