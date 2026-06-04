use crate::tui::render::display::safe_text::{col_to_char_idx, safe_char_slice};
use crate::tui::render::status::{StatusBar, StatusBarRow};
use crate::tui::render::theme;
use crate::tui::view_model::StatusViewModel;
use crate::tui::view_state::StatusSelectionViewState;
use ratatui::{style::Style, text::Span};

impl StatusBar {
    pub fn selected_text_for_view(
        &self,
        view: &StatusSelectionViewState,
        status: &StatusViewModel,
    ) -> Option<String> {
        let (start, end) = view.selection_range()?;
        self.selected_text_for_range(start, end, view.selection_row, view.selection_width, status)
    }

    fn selected_text_for_range(
        &self,
        start: usize,
        end: usize,
        row: StatusBarRow,
        width: u16,
        status: &StatusViewModel,
    ) -> Option<String> {
        let full = self.line_text(row, width, status);
        let chars: Vec<char> = full.chars().collect();
        let selected: String = chars[start.min(chars.len())..end.min(chars.len())]
            .iter()
            .collect();
        if selected.is_empty() {
            None
        } else {
            Some(selected)
        }
    }

    pub(crate) fn spans_with_selection(
        &self,
        full_text: String,
        base: Style,
        view: &StatusSelectionViewState,
    ) -> Vec<Span<'static>> {
        let Some((start, end)) = view.selection_range() else {
            return vec![Span::styled(full_text, base)];
        };
        let chars: Vec<char> = full_text.chars().collect();
        let len = chars.len();
        let before: String = safe_char_slice(&chars, 0, start.min(len)).iter().collect();
        let selected: String = safe_char_slice(&chars, start.min(len), end.min(len))
            .iter()
            .collect();
        let after: String = safe_char_slice(&chars, end.min(len), len).iter().collect();
        let selection_style = Style::default()
            .bg(theme::SELECTION_BG)
            .fg(theme::SELECTION_FG);
        let mut highlighted = Vec::new();
        if !before.is_empty() {
            highlighted.push(Span::styled(before, base));
        }
        if !selected.is_empty() {
            highlighted.push(Span::styled(selected, selection_style));
        }
        if !after.is_empty() {
            highlighted.push(Span::styled(after, base));
        }
        highlighted
    }

    pub(crate) fn screen_col_to_char_idx(
        &self,
        row: StatusBarRow,
        col: u16,
        width: u16,
        status: &StatusViewModel,
    ) -> usize {
        col_to_char_idx(&self.line_text(row, width, status), col as usize)
    }

    /// 只读折算：把状态栏屏幕坐标 `(row, col)`（已相对 `status_bar_rect`）折算成
    /// view_state 选区锚点 `(StatusBarRow, char_idx, width)`，**不改 widget 选区字段**。
    ///
    /// `bar_y`/`bar_x`/`bar_width` 为 render 期 `status_bar_rect` 的几何（由 mouse_handler
    /// 据当前 layout 传入）。逻辑搬自 `mouse_handler` 的 Down/status 分支：
    /// - `row == bar_y + 1` 判定为 Context 行，否则 Runtime 行；
    /// - 列相对 `bar_x` 偏移后经 `screen_col_to_char_idx` 折算成 plain 文本 char_idx
    ///   （依赖 render 期 `build_full_text`/`context_row_text`，故留 widget 只读借用，
    ///   对齐 output 的 `screen_to_anchor`）。
    pub(crate) fn screen_to_status_anchor(
        &self,
        row: u16,
        col: u16,
        bar_y: u16,
        bar_x: u16,
        bar_width: u16,
        status: &StatusViewModel,
    ) -> (StatusBarRow, usize, u16) {
        let status_row = if row == bar_y.saturating_add(1) {
            StatusBarRow::Context
        } else {
            StatusBarRow::Runtime
        };
        let char_idx =
            self.screen_col_to_char_idx(status_row, col.saturating_sub(bar_x), bar_width, status);
        (status_row, char_idx, bar_width)
    }

    pub(crate) fn line_text(
        &self,
        row: StatusBarRow,
        width: u16,
        status: &StatusViewModel,
    ) -> String {
        match row {
            StatusBarRow::Runtime => self.build_full_text(status),
            StatusBarRow::Context => self.context_row_text_for_view(width as usize, status),
        }
    }
}
