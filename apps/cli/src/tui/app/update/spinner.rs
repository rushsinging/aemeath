use crate::tui::app::App;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;

impl App {
    /// 设置 spinner phase（自动置 active）。
    pub(crate) fn spinner_phase(&mut self, phase: SpinnerPhase) {
        self.model
            .runtime
            .apply(RuntimeIntent::SetSpinnerPhase(phase));
    }

    /// 停止 spinner（幂等）。
    pub(crate) fn spinner_stop(&mut self) {
        self.model.runtime.apply(RuntimeIntent::StopSpinner);
    }
}

pub(super) fn truncate_for_spinner(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

pub(super) fn short_hook_command(command: &str) -> String {
    let trimmed = command.trim().trim_matches('"');
    let tail = trimmed.rsplit('/').next().unwrap_or(trimmed);
    truncate_for_spinner(tail, 48)
}
