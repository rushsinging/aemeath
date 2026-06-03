use crate::tui::model::input::change::InputChange;
use crate::tui::view_state::InputSelectionViewState;
use crate::tui::{InputArea, StatusBar};

pub(crate) fn apply_input_changes_to_widget(
    input_area: &mut InputArea,
    _status_bar: &mut StatusBar,
    changes: &[InputChange],
) -> Option<String> {
    let mut submission = None;
    for change in changes {
        match change {
            InputChange::TextChanged { .. }
            | InputChange::HistorySelected { .. }
            | InputChange::CursorMoved { .. }
            | InputChange::CompletionChanged { .. }
            | InputChange::AttachmentChanged { .. } => {}
            InputChange::Submitted {
                submission: submitted,
            } => {
                submission = Some(submitted.text.clone());
                input_area.clear();
            }
            InputChange::Cleared => {
                input_area.clear();
            }
            InputChange::ModeChanged { .. } => {}
        }
    }
    submission
}

/// 据 view_state 选区真相单向写回 widget input 选区镜像（#59 S4，仿
/// `output_view_widget.rs::apply_output_selection_to_widget` /
/// `status_widget.rs::apply_status_selection_to_widget`）。
///
/// `view_state.input_sel` 是 input 选区真相（input text `(row, col)` 锚点状态机），
/// widget 的 `is_selecting`/`selection_start`/`selection_end` 降为渲染高亮镜像。
///
/// 每帧渲染前调用；复制选中文本直接读取 `InputSelectionViewState` + input document text，
/// 不再把 widget 选区镜像作为取文真相。
pub(crate) fn apply_input_selection_to_widget(
    view: &InputSelectionViewState,
    input_area: &mut InputArea,
) {
    input_area.apply_selection_mirror(view.is_selecting, view.selection_start, view.selection_end);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_input_changes_to_widget_ignores_text_change_mirror() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::TextChanged {
            text: "abc".to_string(),
            cursor: 3,
        }];

        let submitted = apply_input_changes_to_widget(&mut input_area, &mut status_bar, &changes);

        assert_eq!(submitted, None);
    }

    #[test]
    fn test_apply_input_selection_writes_view_anchors_to_widget() {
        let mut input_area = InputArea::new();
        let text = "你好a";
        let mut view = InputSelectionViewState::default();
        view.begin_selection((0, 1));
        view.update_selection((0, 3));

        apply_input_selection_to_widget(&view, &mut input_area);

        assert!(input_area.is_selecting());
        assert_eq!(
            input_area.get_selected_text_for_text(text),
            Some("好a".to_string())
        );
    }

    #[test]
    fn test_apply_input_selection_clears_widget_when_view_empty() {
        let mut input_area = InputArea::new();
        let text = "hello";
        input_area.apply_selection_mirror(true, Some((0, 0)), Some((0, 5)));
        assert!(input_area.is_selecting());

        let view = InputSelectionViewState::default();
        apply_input_selection_to_widget(&view, &mut input_area);

        assert!(!input_area.is_selecting());
        assert!(input_area.get_selected_text_for_text(text).is_none());
    }

    #[test]
    fn test_apply_input_changes_to_widget_returns_submission() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::Submitted {
            submission: crate::tui::model::input::submission::InputSubmission {
                text: "run".to_string(),
                attachments: Vec::new(),
            },
        }];

        let submitted = apply_input_changes_to_widget(&mut input_area, &mut status_bar, &changes);

        assert_eq!(submitted.as_deref(), Some("run"));
        assert!(!input_area.is_selecting());
    }
}
