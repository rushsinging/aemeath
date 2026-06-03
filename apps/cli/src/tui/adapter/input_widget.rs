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
            InputChange::TextChanged { text, cursor }
            | InputChange::HistorySelected { text, cursor } => {
                input_area.set_text(text);
                input_area.set_cursor_byte_index(*cursor);
            }
            InputChange::CursorMoved { cursor } => {
                input_area.set_cursor_byte_index(*cursor);
            }
            InputChange::CompletionChanged { .. } => {}
            InputChange::Submitted {
                submission: submitted,
            } => {
                submission = Some(submitted.text.clone());
                input_area.clear();
            }
            InputChange::Cleared => {
                input_area.clear();
            }
            InputChange::AttachmentChanged { count } => {
                input_area.set_pending_images(*count);
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
/// `view_state.input_sel` 是 input 选区真相（textarea `(row, col)` 锚点状态机），
/// widget 的 `is_selecting`/`selection_start`/`selection_end` 降为只读镜像，供 render
/// 期高亮与 `get_selected_text` 取 plain 文本。这是这些镜像字段的唯一生产写入路径。
///
/// 每帧渲染前调用；mouse-up 复制前亦显式调用以消除一帧滞后（对齐 output/status 选区
/// 时序）。T4 接线。
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
    fn test_apply_input_changes_to_widget_applies_text_change() {
        let mut input_area = InputArea::new();
        let mut status_bar = StatusBar::new();
        let changes = vec![InputChange::TextChanged {
            text: "abc".to_string(),
            cursor: 3,
        }];

        apply_input_changes_to_widget(&mut input_area, &mut status_bar, &changes);

        assert_eq!(input_area.get_text(), "abc");
    }

    #[test]
    fn test_apply_input_selection_writes_view_anchors_to_widget() {
        let mut input_area = InputArea::new();
        input_area.set_text("你好a");
        // render 期 textarea 折算（widget textarea_pos）取得 (row, col) 锚点，
        // 这里直接构造已折算的 view_state 选区覆盖 "好a"（字符索引 1..3）。
        let mut view = InputSelectionViewState::default();
        view.begin_selection((0, 1));
        view.update_selection((0, 3));

        apply_input_selection_to_widget(&view, &mut input_area);

        // 正常路径：镜像写回后 is_selecting 置位，经 widget plain 取到选中文本。
        assert!(input_area.is_selecting());
        assert_eq!(input_area.get_selected_text(), Some("好a".to_string()));
    }

    #[test]
    fn test_apply_input_selection_clears_widget_when_view_empty() {
        let mut input_area = InputArea::new();
        input_area.set_text("hello");
        // widget 先持有旧镜像，模拟上一帧选区（经 adapter 唯一生产写入路径写回）。
        input_area.apply_selection_mirror(true, Some((0, 0)), Some((0, 5)));
        assert!(input_area.is_selecting());

        // view_state 为空（默认）→ 镜像被清空。
        let view = InputSelectionViewState::default();
        apply_input_selection_to_widget(&view, &mut input_area);

        // 边界/清空路径：view_state 无选区 → 镜像被清空（is_selecting 关，取不到文本）。
        assert!(!input_area.is_selecting());
        assert!(input_area.get_selected_text().is_none());
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
        assert!(input_area.is_empty());
    }
}
