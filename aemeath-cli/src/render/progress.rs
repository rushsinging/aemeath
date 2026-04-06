use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub struct ThinkingIndicator {
    progress: ProgressBar,
}

impl ThinkingIndicator {
    pub fn start(message: &str) -> Self {
        let progress = ProgressBar::new_spinner();
        progress.enable_steady_tick(Duration::from_millis(100));
        
        let style = ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg:.white}")
            .unwrap();
        
        progress.set_style(style);
        progress.set_message(message.to_string());
        
        Self { progress }
    }

    pub fn stop(self) {
        self.progress.finish_and_clear();
    }
}
