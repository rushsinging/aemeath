use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::root::TuiModel;
use crate::tui::view_assembler::status::StatusViewAssembler;
use crate::tui::StatusBar;

/// 单向写回 StatusBar 运行态镜像：由 `StatusViewAssembler` 从 Model 派生 ViewModel，
/// 再经唯一写入口 `apply_runtime_view` 落地 widget。这是 model/session/tps/token/api/
/// context_size/工作目录上下文的唯一生产写入路径。
pub(crate) fn apply_runtime_status_to_widget(model: &TuiModel, status_bar: &mut StatusBar) {
    let view = StatusViewAssembler::assemble_runtime_view(&model.runtime, Some(&model.session));
    status_bar.apply_runtime_view(view);
}

pub(crate) fn apply_diagnostic_status_to_widget(model: &TuiModel, status_bar: &mut StatusBar) {
    match model.diagnostic.highest_severity() {
        Some(DiagnosticSeverity::Error) => status_bar.set_warning("Error"),
        Some(DiagnosticSeverity::Warning) => status_bar.set_warning("Warning"),
        Some(DiagnosticSeverity::Info) | None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::runtime::intent::RuntimeIntent;
    use crate::tui::model::runtime::session_intent::SessionIntent;
    use crate::tui::model::runtime::workspace::WorktreeKind;

    #[test]
    fn test_apply_runtime_status_writes_model_usage_and_context() {
        let mut model = TuiModel::default();
        model.runtime.apply(RuntimeIntent::SetProviderModel {
            provider: None,
            model_id: Some("glm-5.1".to_string()),
        });
        model.runtime.apply(RuntimeIntent::RecordUsage {
            input_tokens: 12_400,
            output_tokens: 1_800,
            last_input_tokens: 74_000,
            cost_usd: 0.0,
        });
        model.runtime.apply(RuntimeIntent::SetContextSize(200_000));
        model
            .runtime
            .apply(RuntimeIntent::WorkspaceSnapshotReceived {
                path_base: Some("~/repo".to_string()),
                working_root: Some("~/repo".to_string()),
                branch: Some("main".to_string()),
                kind: WorktreeKind::MainCheckout,
            });
        model.session.apply(SessionIntent::SetCurrentSession {
            id: "s-1".to_string(),
        });
        let mut status_bar = StatusBar::new();

        apply_runtime_status_to_widget(&model, &mut status_bar);

        let row = status_bar.build_full_text();
        assert!(row.contains("glm-5.1"));
        assert!(row.contains("in 12k"));
        assert!(row.contains("out 1.8k"));
        assert!(row.contains("ctx 37%"));
        assert!(row.contains("api 1"));
        let context = status_bar.context_row_text(120);
        assert!(context.contains("~/repo"));
        assert!(context.contains("session s-1"));
    }

    #[test]
    fn test_apply_runtime_status_empty_model_keeps_defaults() {
        let model = TuiModel::default();
        let mut status_bar = StatusBar::new();

        apply_runtime_status_to_widget(&model, &mut status_bar);

        let row = status_bar.build_full_text();
        assert!(row.contains("Ready"));
    }

    #[test]
    fn test_apply_diagnostic_status_sets_warning_on_error() {
        let mut model = TuiModel::default();
        model.diagnostic.apply(
            crate::tui::model::diagnostic::intent::DiagnosticIntent::RecordNotice {
                severity: DiagnosticSeverity::Error,
                message: "boom".to_string(),
            },
        );
        let mut status_bar = StatusBar::new();

        apply_diagnostic_status_to_widget(&model, &mut status_bar);

        assert!(status_bar.build_full_text().contains("Error"));
    }
}
