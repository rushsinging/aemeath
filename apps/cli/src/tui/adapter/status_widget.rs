//! Retired StatusBar widget adapter.
//!
//! StatusBar is now stateless for runtime/diagnostic/status data: render and
//! selection receive `StatusViewModel` directly. This module remains only to
//! verify the projection that replaced the old widget writeback adapter.

#[cfg(test)]
mod tests {
    use crate::tui::model::conversation::intent::{
        RecordUsage, SetContextSize, SetProviderModel, SetStatusNotice, SetThinking,
        WorkspaceSnapshotReceived,
    };
    use crate::tui::model::conversation::status_notice::{StatusNotice, StatusNoticeKind};
    use crate::tui::model::conversation::workspace::WorktreeKind;
    use crate::tui::model::diagnostic::intent::DiagnosticIntent;
    use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
    use crate::tui::model::root::TuiModel;
    use crate::tui::model::runtime::session_intent::SessionIntent;
    use crate::tui::view_assembler::status::StatusViewAssembler;
    use crate::tui::view_model::{StatusNoticeViewKind, StatusSeverity};

    #[test]
    fn test_status_view_projects_runtime_usage_and_context() {
        let mut model = TuiModel::default();
        model.conversation.apply(SetProviderModel {
            provider: None,
            model_id: Some("glm-5.1".to_string()),
        });
        model.conversation.apply(RecordUsage {
            input_tokens: 12_400,
            output_tokens: 1_800,
            last_input_tokens: 74_000,
            cost_usd: 0.0,
        });
        model.conversation.apply(SetContextSize(200_000));
        model.conversation.apply(WorkspaceSnapshotReceived {
            path_base: Some("~/repo".to_string()),
            workspace_root: Some("~/repo".to_string()),
            branch: Some("main".to_string()),
            kind: WorktreeKind::MainCheckout,
        });
        model.session.apply(SessionIntent::SetCurrentSession {
            id: "s-1".to_string(),
        });
        model.conversation.apply(SetThinking(true));

        let view = StatusViewAssembler::assemble_status_view(
            &model.conversation,
            Some(&model.session),
            &model.diagnostic,
        );

        assert_eq!(view.runtime.model.as_deref(), Some("glm-5.1"));
        assert_eq!(view.runtime.input_tokens, 12_400);
        assert_eq!(view.runtime.output_tokens, 1_800);
        assert_eq!(view.runtime.last_input_tokens, 74_000);
        assert_eq!(view.runtime.context_size, 200_000);
        assert_eq!(view.runtime.api_calls, 1);
        assert_eq!(view.runtime.session_id.as_deref(), Some("s-1"));
        assert_eq!(view.runtime.context.path_base, "~/repo");
        assert_eq!(view.runtime.context.branch.as_deref(), Some("main"));
        assert!(view.thinking);
    }

    #[test]
    fn test_status_view_projects_status_notice() {
        let mut model = TuiModel::default();
        model
            .conversation
            .apply(SetStatusNotice(StatusNotice::warning("Interrupted")));
        model.conversation.apply(SetThinking(false));

        let view =
            StatusViewAssembler::assemble_status_view(&model.conversation, None, &model.diagnostic);

        assert_eq!(view.notice.text, "Interrupted");
        assert_eq!(view.notice.kind, StatusNoticeViewKind::Warning);
        assert!(!view.thinking);
    }

    #[test]
    fn test_status_notice_kind_maps_all_variants() {
        assert_eq!(
            StatusViewAssembler::assemble_notice_view(&StatusNotice::ready()).kind,
            StatusNoticeViewKind::Normal
        );
        assert_eq!(
            StatusViewAssembler::assemble_notice_view(&StatusNotice::success("Copied")).kind,
            StatusNoticeViewKind::Success
        );
        assert_eq!(
            StatusViewAssembler::assemble_notice_view(&StatusNotice {
                text: "Warn".to_string(),
                kind: StatusNoticeKind::Warning,
            })
            .kind,
            StatusNoticeViewKind::Warning
        );
    }

    #[test]
    fn test_status_view_projects_diagnostic_severity() {
        let mut model = TuiModel::default();
        model.diagnostic.apply(DiagnosticIntent::RecordNotice {
            severity: DiagnosticSeverity::Error,
            message: "boom".to_string(),
        });

        let view =
            StatusViewAssembler::assemble_status_view(&model.conversation, None, &model.diagnostic);

        assert_eq!(view.line.severity, StatusSeverity::Error);
    }
}
