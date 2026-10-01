use crate::tui::model::conversation::intent::RecordLiveTps;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::workspace::WorktreeKind;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime_presentation::{RuntimePresentation, RuntimePresentationIntent};
use crate::tui::model::workspace_provider::{WorkspaceIntent, WorkspaceProvider};

use super::StatusViewAssembler;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::runtime::session_model::SessionModel;

#[test]
fn test_assemble_runtime_view_normal_path_derives_all_fields() {
    let mut conversation = ConversationModel::default();
    let mut presentation = RuntimePresentation::default();
    presentation.apply(RuntimePresentationIntent::ProviderModel {
        provider: None,
        model_id: Some("glm-5.1".to_string()),
    });
    conversation.apply(RecordLiveTps { tps: 42.0 });
    let mut workspace = WorkspaceProvider::default();
    workspace.apply(WorkspaceIntent::ApplySnapshot {
        path_base: Some("~/repo/cli".to_string()),
        workspace_root: Some("~/repo".to_string()),
    });
    workspace.apply(WorkspaceIntent::ApplyMetadata {
        root: "~/repo".to_string(),
        revision: 1,
        branch: Some("feature/x".to_string()),
        kind: WorktreeKind::LinkedWorktree,
    });
    let mut session = SessionModel::default();
    session.apply(SessionIntent::SetCurrentSession {
        id: "s-1".to_string(),
    });

    let vm = StatusViewAssembler::assemble_runtime_view(
        &conversation,
        &presentation,
        &workspace,
        Some(&session),
        "ask",
    );

    assert_eq!(vm.model.as_deref(), Some("glm-5.1"));
    assert_eq!(vm.session_id.as_deref(), Some("s-1"));
    assert_eq!(vm.tps, 42.0);
    assert_eq!(vm.context.path_base, "~/repo/cli");
    assert_eq!(vm.context.branch.as_deref(), Some("feature/x"));
    assert_eq!(
        vm.context.kind,
        crate::tui::view_model::StatusWorktreeKind::Worktree
    );
}

#[test]
fn test_assemble_runtime_view_boundary_empty_branch_becomes_none() {
    let conversation = ConversationModel::default();
    let presentation = RuntimePresentation::default();
    let mut workspace = WorkspaceProvider::default();
    workspace.apply(WorkspaceIntent::ApplySnapshot {
        path_base: Some("/repo".to_string()),
        workspace_root: Some("/repo".to_string()),
    });
    workspace.apply(WorkspaceIntent::ApplyMetadata {
        root: "/repo".to_string(),
        revision: 1,
        branch: Some("   ".to_string()),
        kind: WorktreeKind::MainCheckout,
    });

    let vm = StatusViewAssembler::assemble_runtime_view(
        &conversation,
        &presentation,
        &workspace,
        None,
        "ask",
    );

    assert!(vm.context.branch.is_none());
    assert_eq!(
        vm.context.kind,
        crate::tui::view_model::StatusWorktreeKind::Main
    );
}

#[test]
fn test_assemble_runtime_view_error_path_missing_model_and_session() {
    let conversation = ConversationModel::default();
    let presentation = RuntimePresentation::default();
    let workspace = WorkspaceProvider::default();

    let vm = StatusViewAssembler::assemble_runtime_view(
        &conversation,
        &presentation,
        &workspace,
        None,
        "ask",
    );

    assert!(vm.model.is_none());
    assert!(vm.session_id.is_none());
    assert_eq!(vm.tps, 0.0);
    assert!(vm.context.path_base.is_empty());
}

#[test]
fn test_status_assembler_reads_runtime_and_diagnostic() {
    let conversation = ConversationModel::default();
    let mut presentation = RuntimePresentation::default();
    presentation.apply(RuntimePresentationIntent::ProviderModel {
        provider: None,
        model_id: Some("gpt-5.5".to_string()),
    });
    let mut workspace = WorkspaceProvider::default();
    workspace.apply(WorkspaceIntent::SetCurrent {
        cwd: "/repo".to_string(),
        worktree: None,
    });

    let mut diagnostic = DiagnosticModel::default();
    diagnostic.apply(DiagnosticIntent::RecordNotice {
        severity: DiagnosticSeverity::Warning,
        message: "orphan event".to_string(),
    });

    let vm = StatusViewAssembler::assemble_from_runtime_session(
        &conversation,
        &presentation,
        &workspace,
        None,
        &diagnostic,
    );
    assert!(vm.left.iter().any(|segment| segment.text == "gpt-5.5"));
    assert!(vm.right.iter().any(|segment| segment.text == "/repo"));
    assert!(vm
        .center
        .iter()
        .any(|segment| segment.text.contains("warning")));
}
