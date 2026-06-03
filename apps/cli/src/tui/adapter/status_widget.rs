use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::root::TuiModel;
use crate::tui::view_assembler::status::StatusViewAssembler;
use crate::tui::view_state::StatusSelectionViewState;
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

/// 据 view_state 选区真相单向写回 widget status 选区镜像（#59 S4，仿
/// `output_view_widget.rs::apply_output_selection_to_widget`）。
///
/// `view_state.status_sel` 是 status 选区真相（char_idx 锚点状态机 + row/width 折算
/// 上下文），widget 的 `is_selecting`/`selection_start`/`selection_end`/`selection_row`/
/// `selection_width` 降为只读镜像，供 render 期 `spans_with_selection` 高亮与
/// `get_selected_text` 取 plain 文本。这是这些镜像字段的唯一生产写入路径。
///
/// 每帧渲染前由 `refresh_output_scroll_from_view_state` 调用；mouse-up 复制前亦显式
/// 调用以消除一帧滞后（对齐 output 选区时序）。
pub(crate) fn apply_status_selection_to_widget(
    view: &StatusSelectionViewState,
    status_bar: &mut StatusBar,
) {
    status_bar.apply_selection_mirror(
        view.is_selecting,
        view.selection_start,
        view.selection_end,
        view.selection_row,
        view.selection_width,
    );
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

    #[test]
    fn test_apply_status_selection_writes_view_anchors_to_widget() {
        use crate::tui::render::status::StatusBarRow;
        // 让 Runtime 行有可选文本（"glm-5.1" 等），便于校验 plain 折算后取文本。
        let mut model = TuiModel::default();
        model.runtime.apply(
            crate::tui::model::runtime::intent::RuntimeIntent::SetProviderModel {
                provider: None,
                model_id: Some("glm-5.1".to_string()),
            },
        );
        let mut status_bar = StatusBar::new();
        apply_runtime_status_to_widget(&model, &mut status_bar);
        let full = status_bar.build_full_text();
        let char_len = full.chars().count();
        assert!(char_len >= 2, "Runtime 行应有可选文本");

        // view_state 选区覆盖整行，单向写回 widget 镜像。
        let mut view = StatusSelectionViewState::default();
        view.begin_selection(StatusBarRow::Runtime, 0, 0);
        view.update_selection(char_len);

        apply_status_selection_to_widget(&view, &mut status_bar);

        // 正常路径：镜像写回后 is_selecting 置位，且经 widget plain 折算取到整行文本。
        assert!(status_bar.is_selecting());
        assert_eq!(status_bar.get_selected_text(), Some(full));
    }

    #[test]
    fn test_apply_status_selection_clears_widget_when_view_empty() {
        use crate::tui::render::status::StatusBarRow;
        // widget 先持有旧镜像，模拟上一帧选区（经 adapter 唯一生产写入路径写回）。
        let mut status_bar = StatusBar::new();
        status_bar.apply_selection_mirror(true, Some(0), Some(50), StatusBarRow::Runtime, 0);
        assert!(status_bar.is_selecting());

        // view_state 为空（默认）→ 镜像被清空。
        let view = StatusSelectionViewState::default();
        apply_status_selection_to_widget(&view, &mut status_bar);

        // 边界/清空路径：view_state 无选区 → 镜像被清空（is_selecting 关，取不到文本）。
        assert!(!status_bar.is_selecting());
        assert!(status_bar.get_selected_text().is_none());
    }
}
