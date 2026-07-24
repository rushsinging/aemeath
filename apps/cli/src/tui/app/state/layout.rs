//! 布局与对话框状态

use crate::tui::app::state::TerminalSize;
use crate::tui::render::dialog::Dialog;
use ratatui::layout::Rect;

/// UI 布局与对话框相关状态
#[derive(Default)]
pub(crate) struct UiLayout {
    pub should_exit: bool,
    pub output_area_rect: Rect,
    pub input_area_rect: Rect,
    pub status_bar_rect: Rect,
    pub last_terminal_size: Option<TerminalSize>,
    pub active_dialog: Option<Dialog>,
    pub dialog_model_keys: Vec<String>,
    /// Interaction overlay cursor (for option selection within AskUserQuestion etc.)
    pub interaction_selected: usize,
    /// Ctrl+C 第一次按下的时间（用于 double-CtrlC 退出）
    pub last_ctrlc: Option<std::time::Instant>,
}

impl UiLayout {
    pub(crate) fn request_exit(&mut self) {
        self.should_exit = true;
    }

    pub(crate) fn clear_ctrlc(&mut self) {
        self.last_ctrlc = None;
    }

    pub(crate) fn mark_ctrlc_now(&mut self) {
        self.last_ctrlc = Some(std::time::Instant::now());
    }

    pub(crate) fn has_active_dialog(&self) -> bool {
        self.active_dialog.is_some()
    }

    pub(crate) fn active_dialog(&self) -> Option<&Dialog> {
        self.active_dialog.as_ref()
    }

    pub(crate) fn active_dialog_mut(&mut self) -> Option<&mut Dialog> {
        self.active_dialog.as_mut()
    }

    pub(crate) fn selected_model_key(&self) -> Option<String> {
        let selected = self.active_dialog.as_ref()?.get_selected()?;
        self.dialog_model_keys.get(selected).cloned()
    }

    pub(crate) fn clear_dialog(&mut self) {
        self.active_dialog = None;
        self.dialog_model_keys.clear();
    }

    pub(crate) fn open_model_dialog(&mut self, dialog: Dialog, model_keys: Vec<String>) {
        self.active_dialog = Some(dialog);
        self.dialog_model_keys = model_keys;
    }

    pub(crate) fn update_areas(
        &mut self,
        output_area_rect: Rect,
        input_area_rect: Rect,
        status_bar_rect: Rect,
    ) {
        self.output_area_rect = output_area_rect;
        self.input_area_rect = input_area_rect;
        self.status_bar_rect = status_bar_rect;
    }
}
