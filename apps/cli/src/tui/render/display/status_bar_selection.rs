use crate::tui::render::display::safe_text::{col_to_char_idx, safe_char_slice};
use crate::tui::render::status::{StatusBar, StatusBarRow};
use crate::tui::render::theme;
use ratatui::{style::Style, text::Span};

impl StatusBar {
    #[allow(dead_code)]
    pub fn start_selection(&mut self, col: u16) {
        self.start_selection_at(StatusBarRow::Runtime, col, 0);
    }

    pub fn start_selection_at(&mut self, row: StatusBarRow, col: u16, width: u16) {
        self.selection_row = row;
        self.selection_width = width;
        self.selection_start = Some(self.screen_col_to_char_idx(row, col, width));
        self.selection_end = Some(self.screen_col_to_char_idx(row, col, width));
        self.is_selecting = true;
    }

    #[allow(dead_code)]
    pub fn update_selection(&mut self, col: u16) {
        self.update_selection_at(col, 0);
    }

    pub fn update_selection_at(&mut self, col: u16, width: u16) {
        if self.is_selecting {
            self.selection_end = Some(self.screen_col_to_char_idx(self.selection_row, col, width));
        }
    }

    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        self.selection_start = None;
        self.selection_end = None;
        self.selection_width = 0;
        text
    }

    pub fn get_selected_text(&self) -> Option<String> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        let (start, end) = ordered_range(start, end)?;
        let full = self.line_text(self.selection_row, self.selection_width);
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
    ) -> Vec<Span<'static>> {
        let (Some(start), Some(end)) = (self.selection_start, self.selection_end) else {
            return vec![Span::styled(full_text, base)];
        };
        let Some((start, end)) = ordered_range(start, end) else {
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

    pub(crate) fn screen_col_to_char_idx(&self, row: StatusBarRow, col: u16, width: u16) -> usize {
        col_to_char_idx(&self.line_text(row, width), col as usize)
    }

    pub(crate) fn line_text(&self, row: StatusBarRow, width: u16) -> String {
        match row {
            StatusBarRow::Runtime => self.build_full_text(),
            StatusBarRow::Context => self.context_row_text(width as usize),
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.selection_row = StatusBarRow::Runtime;
        self.selection_width = 0;
        self.is_selecting = false;
    }

    /// 由 adapter（`apply_status_selection_to_widget`）单向写回 status 选区镜像。
    ///
    /// #59 S4：选区真相在 `view_state::StatusSelectionViewState`，widget 的
    /// `is_selecting`/`selection_*` 降为只读镜像，供 render 期 `spans_with_selection`
    /// 高亮与 `get_selected_text` 取 plain 文本。这是这些镜像字段的**唯一**生产写入
    /// 路径（widget 内部 `clear_selection`/`reset_runtime_state` 与测试除外）。T2 接线。
    pub(crate) fn apply_selection_mirror(
        &mut self,
        is_selecting: bool,
        selection_start: Option<usize>,
        selection_end: Option<usize>,
        selection_row: StatusBarRow,
        selection_width: u16,
    ) {
        self.is_selecting = is_selecting;
        self.selection_start = selection_start;
        self.selection_end = selection_end;
        self.selection_row = selection_row;
        self.selection_width = selection_width;
    }

    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }
}

fn ordered_range(start: usize, end: usize) -> Option<(usize, usize)> {
    let (start, end) = if start < end {
        (start, end)
    } else {
        (end, start)
    };
    if start == end {
        None
    } else {
        Some((start, end))
    }
}
