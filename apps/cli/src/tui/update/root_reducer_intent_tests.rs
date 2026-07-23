use super::*;
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::{
    ClearCompactRuntime, ConfirmInteraction, ConversationIntent, RunAwaitingUser, RunStarted,
    SetSpinnerPhase, ShowInteraction, StartChat, StopSpinner, SyncQueuedSubmissions,
    UpdateInteractionDraft,
};
use crate::tui::model::conversation::interaction::{
    InteractionBody, InteractionDraftAction, InteractionRequest, UiInteractionRequestId, UiRunId,
    UiStuckDiagnostic,
};
use crate::tui::model::conversation::spinner::SpinnerPhase;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::workspace_provider::WorkspaceIntent;
use crate::tui::update::intent::AgentIntent;

#[test]
fn interaction_confirmation_marks_output_dirty_and_emits_reply_effect() {
    let mut model = TuiModel::default();
    let request_id = UiInteractionRequestId::from("request-1");
    reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::ShowInteraction(ShowInteraction {
            request: InteractionRequest {
                request_id: request_id.clone(),
                run_id: UiRunId::from("run-1"),
                body: InteractionBody::HardPause(UiStuckDiagnostic {
                    reason: "等待确认".to_string(),
                    recent_actions: Vec::new(),
                }),
            },
        })),
    );
    reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::UpdateInteractionDraft(
            UpdateInteractionDraft {
                request_id: request_id.clone(),
                action: InteractionDraftAction::ContinueHardPause,
            },
        )),
    );

    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::ConfirmInteraction(ConfirmInteraction {
            request_id,
        })),
    );

    assert!(result.dirty.output);
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::ReplyInteraction { reply: crate::tui::model::conversation::interaction::UiInteractionReply::ContinueHardPause, .. }, Effect::RequestRender]
            | [Effect::RequestRender, Effect::ReplyInteraction { reply: crate::tui::model::conversation::interaction::UiInteractionReply::ContinueHardPause, .. }]
    ));
}

#[test]
fn conversation_intent_starts_chat_and_marks_output_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::StartChat(StartChat {
            submission: "hello".to_string(),
        })),
    );

    assert_eq!(model.conversation.chats.len(), 1);
    assert!(result.dirty.output);
    assert_eq!(
        result
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::RequestRender))
            .count(),
        1
    );
}

#[test]
fn input_intent_marks_only_input_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Input(InputIntent::InsertText("hello".to_string())),
    );

    assert_eq!(model.input.document.buffer, "hello");
    assert!(result.dirty.input);
    assert!(matches!(
        result.input_changes.as_slice(),
        [crate::tui::model::input::change::InputChange::TextChanged { text, .. }] if text == "hello"
    ));
    assert!(!result.dirty.output);
    assert!(!result.dirty.status);
    assert!(!result.dirty.dialog);
}

#[test]
fn runtime_presentation_intent_updates_model_and_marks_status_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::RuntimePresentation(
            crate::tui::model::runtime_presentation::RuntimePresentationIntent::ProviderModel {
                provider: Some("anthropic".to_string()),
                model_id: Some("claude-opus".to_string()),
            },
        ),
    );

    assert_eq!(model.runtime_presentation.provider(), Some("anthropic"));
    assert_eq!(model.runtime_presentation.model_id(), Some("claude-opus"));
    assert!(result.dirty.status);
}

#[test]
fn workspace_snapshot_intent_marks_output_and_status_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Workspace(WorkspaceIntent::ApplySnapshot {
            path_base: Some("/repo".to_string()),
            workspace_root: Some("/repo".to_string()),
        }),
    );

    assert_eq!(model.workspace_provider.workspace_root(), Some("/repo"));
    assert_eq!(model.workspace_provider.revision(), 1);
    assert!(result.dirty.output);
    assert!(result.dirty.status);
    assert!(matches!(
        result.effects.as_slice(),
        [Effect::ResolveWorkspaceMetadata { root, revision: 1 }, Effect::RequestRender]
            | [Effect::RequestRender, Effect::ResolveWorkspaceMetadata { root, revision: 1 }]
            if root == "/repo"
    ));
}

#[test]
fn matching_workspace_metadata_marks_status_without_triggering_metadata_effect() {
    let mut model = TuiModel::default();
    reduce_intent(
        &mut model,
        AgentIntent::Workspace(WorkspaceIntent::ApplySnapshot {
            path_base: Some("/repo".to_string()),
            workspace_root: Some("/repo".to_string()),
        }),
    );

    let result = reduce_intent(
        &mut model,
        AgentIntent::Workspace(WorkspaceIntent::ApplyMetadata {
            root: "/repo".to_string(),
            revision: 1,
            branch: Some("main".to_string()),
            kind: crate::tui::model::conversation::workspace::WorktreeKind::MainCheckout,
        }),
    );

    assert!(result.dirty.status);
    assert!(!result.dirty.output);
    assert!(!result
        .effects
        .iter()
        .any(|effect| matches!(effect, Effect::ResolveWorkspaceMetadata { .. })));
}

#[test]
fn agent_run_lifecycle_marks_output_dirty_without_command_effect() {
    let mut model = TuiModel::default();
    let run_id = UiRunId::from("run-1");

    let started = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::RunStarted(RunStarted {
            run_id: run_id.clone(),
        })),
    );
    let awaiting = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::RunAwaitingUser(RunAwaitingUser {
            run_id,
        })),
    );

    for result in [started, awaiting] {
        assert!(result.dirty.output);
        assert!(result.dirty.status);
        assert_eq!(
            result
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::RequestRender))
                .count(),
            1
        );
        assert_eq!(result.effects.len(), 1);
    }
}

#[test]
fn ignored_agent_run_transition_is_not_dirty_or_rendered() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::RunAwaitingUser(RunAwaitingUser {
            run_id: UiRunId::from("unknown-run"),
        })),
    );

    assert!(!result.dirty.output);
    assert!(!result.dirty.status);
    assert!(result.effects.is_empty());
}

#[test]
fn diagnostic_intent_marks_status_and_dialog_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Diagnostic(DiagnosticIntent::RecordNotice {
            severity: DiagnosticSeverity::Warning,
            message: "warning".to_string(),
        }),
    );

    assert_eq!(model.diagnostic.notices.len(), 1);
    assert!(result.dirty.status);
    assert!(result.dirty.dialog);
}

#[test]
fn spinner_phase_intent_activates_spinner_and_marks_status_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::SetSpinnerPhase(SetSpinnerPhase {
            phase: SpinnerPhase::Compacting,
        })),
    );

    assert!(model.conversation.runtime.spinner.chat_active);
    assert_eq!(
        model.conversation.runtime.spinner.phase,
        Some(SpinnerPhase::Compacting)
    );
    assert!(result.dirty.status);
}

#[test]
fn stop_spinner_intent_clears_spinner_state_and_marks_status_dirty() {
    let mut model = TuiModel::default();
    model.conversation.runtime.spinner.chat_active = true;
    model.conversation.runtime.spinner.phase = Some(SpinnerPhase::Compacting);
    model.conversation.runtime.spinner.running_tool_count = 2;

    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::StopSpinner(StopSpinner)),
    );

    assert!(!model.conversation.runtime.spinner.chat_active);
    assert_eq!(model.conversation.runtime.spinner.phase, None);
    assert_eq!(model.conversation.runtime.spinner.running_tool_count, 0);
    assert!(result.dirty.status);
}

#[test]
fn queued_snapshot_intent_replaces_queue_bumps_revision_and_marks_output_dirty() {
    let mut model = TuiModel::default();
    let before_revision = model.conversation.revision();

    let input_id = sdk::InputId::new_v7();
    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::SyncQueuedSubmissions(
            SyncQueuedSubmissions {
                queued: vec![sdk::ChatMessage {
                    role: "user".to_string(),
                    content: vec![sdk::ContentBlock::Text {
                        text: "queued".to_string(),
                    }],
                    input_id: Some(input_id.clone()),
                    metadata: None,
                }],
            },
        )),
    );

    assert_eq!(model.conversation.queued_submissions.len(), 1);
    assert_eq!(model.conversation.queued_submissions[0].input_id, input_id);
    assert_eq!(model.conversation.revision(), before_revision + 1);
    assert!(result.dirty.output);
}

#[test]
fn clear_compact_runtime_intent_clears_progress_and_marks_output_dirty() {
    let mut model = TuiModel::default();
    model
        .conversation
        .runtime
        .set_compact_progress("summarizing".to_string(), Some(1), Some(2));
    model.conversation.runtime.spinner.running_tool_count = 2;

    let result = reduce_intent(
        &mut model,
        AgentIntent::Conversation(ConversationIntent::ClearCompactRuntime(ClearCompactRuntime)),
    );

    assert!(model.conversation.runtime.compact_progress.is_none());
    assert_eq!(model.conversation.runtime.spinner.running_tool_count, 0);
    assert!(result.dirty.output);
}
#[test]
fn session_intent_marks_status_dirty() {
    let mut model = TuiModel::default();

    let result = reduce_intent(
        &mut model,
        AgentIntent::Session(SessionIntent::SetCurrentSession {
            id: "session-1".to_string(),
        }),
    );

    assert_eq!(
        model.session.current_session_id.as_deref(),
        Some("session-1")
    );
    assert!(result.dirty.status);
}
