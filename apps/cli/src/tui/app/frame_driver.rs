use crate::tui::app::App;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::update::msg::TuiMsg;
use tokio::sync::mpsc;

use super::event::UiEvent;
use super::update::UpdateResult;

pub(crate) struct FrameOutcome {
    pub effects: Vec<Effect>,
    pub spawn_effect: Option<SpawnAgentChatEffect>,
    pub pending_slash: Option<String>,
}

impl App {
    pub(crate) fn drive_frame(
        &mut self,
        msg: TuiMsg,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &crate::tui::effect::session::processing::SpawnContextRefs,
    ) -> FrameOutcome {
        let UpdateResult {
            effects,
            spawn_effect,
            pending_slash,
        } = self.update(msg, ui_tx, spawn_refs);
        FrameOutcome {
            effects,
            spawn_effect,
            pending_slash,
        }
    }

    pub(crate) fn prepare_frame(&mut self) {
        self.check_ctrlc_timeout();
        self.flush_dirty_view_models();
        self.refresh_live_status_from_model();
        self.refresh_output_scroll_from_view_state();
    }
}
