use crate::tui::app::App;
use crate::tui::model::conversation::spinner::SpinnerPhase;

impl App {
    /// 设置 spinner phase（自动置 chat_active）。
    pub(crate) fn spinner_phase(&mut self, phase: SpinnerPhase) {
        crate::tui::log_debug!(
            "spinner phase set from={:?} to={:?}",
            self.model.conversation.spinner.phase,
            phase,
        );
        self.model.conversation.spinner.chat_active = true;
        self.model.conversation.spinner.phase = Some(phase);
    }

    /// 停止 spinner（幂等）。
    pub(crate) fn spinner_stop(&mut self) {
        crate::tui::log_debug!(
            "spinner stopped chat_active={} phase={:?}",
            self.model.conversation.spinner.chat_active,
            self.model.conversation.spinner.phase,
        );
        self.model.conversation.spinner.chat_active = false;
        self.model.conversation.spinner.phase = None;
        self.model.conversation.spinner.running_tool_count = 0;
    }
}
