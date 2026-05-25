//! 布局与对话框状态

use ratatui::layout::Rect;
use crate::tui::state::TerminalSize;

/// UI 布局与对话框相关状态
pub(crate) struct UiLayout {
    pub should_exit: bool,
    pub output_area_rect: Rect,
    pub input_area_rect: Rect,
    pub status_bar_rect: Rect,
    pub last_terminal_size: Option<TerminalSize>,
    pub active_dialog: Option<crate::tui::dialog::Dialog>,
    pub dialog_model_keys: Vec<String>,
    /// Ctrl+C 第一次按下的时间（用于 double-CtrlC 退出）
    pub last_ctrlc: Option<std::time::Instant>,
}

impl Default for UiLayout {
    fn default() -> Self {
        Self {
            should_exit: false,
            output_area_rect: Rect::default(),
            input_area_rect: Rect::default(),
            status_bar_rect: Rect::default(),
            last_terminal_size: None,
            active_dialog: None,
            dialog_model_keys: Vec::new(),
            last_ctrlc: None,
        }
    }
}
