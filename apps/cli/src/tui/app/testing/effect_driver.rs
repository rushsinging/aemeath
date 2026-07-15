use crate::tui::app::frame_driver::FrameOutcome;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};

#[derive(Default)]
pub(crate) struct RecordingEffectDriver {
    pub effects: Vec<Effect>,
    pub spawn_effects: Vec<SpawnAgentChatEffect>,
    pub pending_slash: Vec<String>,
}

impl RecordingEffectDriver {
    pub fn record(&mut self, outcome: FrameOutcome) {
        self.effects.extend(outcome.effects);
        if let Some(effect) = outcome.spawn_effect {
            self.spawn_effects.push(effect);
        }
        if let Some(input) = outcome.pending_slash {
            self.pending_slash.push(input);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.spawn_effects.is_empty() && self.pending_slash.is_empty()
    }
}
