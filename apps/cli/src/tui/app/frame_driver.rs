use crate::tui::app::App;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::update::msg::TuiMsg;
use std::time::Instant;
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
        let frame_started_at = Instant::now();
        let output_dirty = self.view_state.dirty.output;
        self.check_ctrlc_timeout();
        let before_assemble_count = self.assemble_count_for_diagnostics();
        let flush_started_at = Instant::now();
        self.flush_dirty_view_models();
        self.pending_flush_duration = flush_started_at.elapsed();
        self.refresh_live_status_from_model();
        self.refresh_output_scroll_from_view_state();
        self.pending_prepare_duration = frame_started_at.elapsed();
        self.pending_frame_started_at = Some(frame_started_at);
        let mut context = self.frame_diagnostic_context(output_dirty);
        context.assemble_calls = self
            .assemble_count_for_diagnostics()
            .saturating_sub(before_assemble_count);
        self.pending_frame_context = Some(context);
    }
}
