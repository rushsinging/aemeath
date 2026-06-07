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
