use crate::tui::app::App;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;

impl App {
    /// 设置 spinner phase（自动置 active）。
    pub(crate) fn spinner_phase(&mut self, phase: SpinnerPhase) {
        log::debug!(
            target: "cli::tui::spinner_flow",
            "spinner phase set from={:?} to={:?}",
            self.model.runtime.spinner.phase,
            phase,
        );
        self.model
            .runtime
            .apply(RuntimeIntent::SetSpinnerPhase(phase));
    }

    /// 停止 spinner（幂等）。
    pub(crate) fn spinner_stop(&mut self) {
        log::debug!(
            target: "cli::tui::spinner_flow",
            "spinner stopped active={} phase={:?}",
            self.model.runtime.spinner.active,
            self.model.runtime.spinner.phase,
        );
        self.model.runtime.apply(RuntimeIntent::StopSpinner);
    }
}
