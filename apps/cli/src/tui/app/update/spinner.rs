use crate::tui::app::App;
use crate::tui::model::conversation::intent::{ConversationIntent, SetSpinnerPhase, StopSpinner};
use crate::tui::model::conversation::spinner::SpinnerPhase;
use crate::tui::update::intent::AgentIntent;

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
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::SetSpinnerPhase(SetSpinnerPhase { phase }),
        ));
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
        self.apply_agent_intent(AgentIntent::Conversation(ConversationIntent::StopSpinner(
            StopSpinner,
        )));
    }
}
