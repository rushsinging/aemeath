use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use tokio::sync::mpsc;

impl App {
    pub(super) fn handle_done(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        elapsed: Option<std::time::Duration>,
    ) -> Option<Effect> {
        if let Some(dur) = elapsed {
            self.output_area.push_done(dur);
        }
        self.output_area.finish_streaming();
        self.output_area.stop_spinner();
        self.chat.stop_processing();
        self.status_bar.set_success("Ready");
        self.maybe_auto_reflect(ui_tx);
        // 异步获取 reminders 并推送 recap 行
        if self.agent_client.is_some() {
            Some(Effect::FetchReminderRecap)
        } else {
            None
        }
    }
}
