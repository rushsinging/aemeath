use crate::tui::app::App;
use crate::tui::model::conversation::spinner::SpinnerPhase;

impl App {
    /// 设置 spinner phase（自动置 chat_active）。
    pub(crate) fn spinner_phase(&mut self, phase: SpinnerPhase) {
        let prev_active = self.model.conversation.runtime.spinner.chat_active;
        let prev_phase = self.model.conversation.runtime.spinner.phase.clone();
        crate::tui::log_info!(
            "[SPINNER_DEBUG] spinner_phase called phase={:?} prev_active={} prev_phase={:?}",
            phase,
            prev_active,
            prev_phase,
        );
        self.model.conversation.runtime.spinner.chat_active = true;
        self.model.conversation.runtime.spinner.phase = Some(phase);
    }

    /// 停止 spinner（幂等）。
    pub(crate) fn spinner_stop(&mut self) {
        let prev_active = self.model.conversation.runtime.spinner.chat_active;
        let prev_phase = self.model.conversation.runtime.spinner.phase.clone();
        crate::tui::log_info!(
            "[SPINNER_DEBUG] spinner_stop called prev_active={} prev_phase={:?}",
            prev_active,
            prev_phase,
        );
        self.model.conversation.runtime.spinner.chat_active = false;
        self.model.conversation.runtime.spinner.phase = None;
        self.model.conversation.runtime.spinner.running_tool_count = 0;
    }
}
